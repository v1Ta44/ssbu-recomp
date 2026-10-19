"""Analyze an opt-in NVN lifecycle event prefix. Reports observations, not a crash root-cause verdict."""
import argparse
from collections import Counter
import json
from pathlib import Path
import struct

HEADER = struct.Struct('<8sIIQQ')
RECORD = struct.Struct('<9QII')
TEXTURE_INIT, PACKAGED_SOURCE, SNAPSHOT_READY, TEXTURE_FINAL, READ_BEGIN, READ_END, POOL_INIT, POOL_FINAL_BEGIN, POOL_FINAL_END, RELEASE_BEGIN, RELEASE_END, SNAPSHOT_RELEASE, TEXTURE_REJECTED = range(1, 14)


def overlaps(a, n, b, m):
    return bool(n and m and a < b + m and b < a + n)



class SourceIndex:
    """Conservative 64 KiB buckets; callers still check exact byte intervals.

    Cap per-resource and per-query bucket walks so unusually large ranges cannot
    expand into millions of bucket entries. Keep finalized textures indexed:
    a later invalid read must still retain the intervening release evidence.
    """
    SHIFT = 16
    MAX_BUCKETS = 1024

    def __init__(self):
        self.buckets, self.locations, self.wide = {}, {}, set()

    def replace(self, host, ranges):
        for key in self.locations.pop(host, ()):
            bucket = self.buckets[key]
            bucket.remove(host)
            if not bucket:
                del self.buckets[key]
        self.wide.discard(host)
        keys = set()
        for addr, size in ranges:
            if not size:
                continue
            first, last = addr >> self.SHIFT, (addr + size - 1) >> self.SHIFT
            if last - first >= self.MAX_BUCKETS:
                self.wide.add(host)
            else:
                keys.update(range(first, last + 1))
        for key in keys:
            self.buckets.setdefault(key, set()).add(host)
        self.locations[host] = keys

    def candidates(self, addr, size):
        if not size:
            return set()
        first, last = addr >> self.SHIFT, (addr + size - 1) >> self.SHIFT
        result = self.wide.copy()
        if last - first < self.MAX_BUCKETS:
            for key in range(first, last + 1):
                result.update(self.buckets.get(key, ()))
        else:
            for key, bucket in self.buckets.items():
                if first <= key <= last:
                    result.update(bucket)
        return result


def records(path):
    with Path(path).open('rb') as stream:
        header = stream.read(HEADER.size)
        if len(header) != HEADER.size:
            raise ValueError('incomplete lifecycle header')
        magic, version, size, frequency, reserved = HEADER.unpack(header)
        if magic != b'NVNLIFE\0' or version != 1 or size != RECORD.size or not frequency:
            raise ValueError('unsupported lifecycle header')
        expected = 1
        while raw := stream.read(RECORD.size):
            if len(raw) != RECORD.size:
                yield {'partial_tail_bytes': len(raw)}
                return
            seq, tick, host, guest, pool, addr, length, detail, caller, event, thread = RECORD.unpack(raw)
            if seq != expected:
                raise ValueError(f'noncontiguous lifecycle sequence: {seq}, expected {expected}')
            if event not in range(1, 14):
                raise ValueError(f'unknown lifecycle event: {event}')
            expected += 1
            yield dict(seq=seq, tick=tick, host=host, guest=guest, pool=pool, addr=addr,
                       size=length, detail=detail, caller=caller, event=event, thread=thread)


def analyze(path):
    pools, textures, reads, releases, finalizers = {}, {}, {}, {}, {}
    findings, observations = [], []
    count = partial = 0
    events, read_modes, read_threads, texture_formats, release_kinds = (Counter() for _ in range(5))
    pool_reuses = 0
    snapshot_use = Counter()
    sources = SourceIndex()

    def note(kind, read, cause, evidence):
        findings.append(dict(kind=kind, texture=hex(read['host']), source=hex(read['addr']),
                             size=read['size'], read_seq=read['seq'], cause_seq=cause,
                             thread=read['thread'], evidence=evidence))

    for r in records(path):
        if 'partial_tail_bytes' in r:
            partial = r['partial_tail_bytes']
            break
        count += 1
        e, seq, host = r['event'], r['seq'], r['host']
        events[e] += 1
        if e == POOL_INIT:
            pool_reuses += r['pool'] in pools
            pools[r['pool']] = dict(r, generation=seq, final=None)
        elif e == TEXTURE_INIT:
            texture_formats[hex(r['detail'])] += 1
            textures[host] = dict(r, owner=pools.get(r['pool']), snapshot=None,
                                  invalidations=[], final=None, rejected=False)
            sources.replace(host, [(r['addr'], r['size'])])
        elif e == PACKAGED_SOURCE and host in textures:
            textures[host]['packaged'] = (r['addr'], r['size'])
            tex = textures[host]
            sources.replace(host, [(tex['addr'], tex['size']), tex['packaged']])
        elif e == SNAPSHOT_READY and host in textures:
            textures[host]['snapshot'] = dict(r, released=None)
        elif e == SNAPSHOT_RELEASE and host in textures:
            snap = textures[host]['snapshot']
            if snap:
                snap['released'] = seq
                for read in reads.values():
                    if read.get('snapshot') is snap and overlaps(read['addr'], read['size'], snap['addr'], snap['size']):
                        note('snapshot_release_overlaps_cpu_read', read, seq,
                             'owned snapshot cleanup recorded while a CPU read of that snapshot remains open')
        elif e in (TEXTURE_FINAL, TEXTURE_REJECTED) and host in textures:
            textures[host]['final'] = seq
            textures[host]['rejected'] = e == TEXTURE_REJECTED
        elif e == POOL_FINAL_BEGIN:
            key = (r['thread'], r['pool'])
            finalizers.setdefault(key, []).append(pools.get(r['pool']))
        elif e == POOL_FINAL_END:
            key = (r['thread'], r['pool'])
            pending = finalizers.get(key)
            if not pending:
                raise ValueError(f'unmatched pool finalization end at sequence {seq}')
            owner = pending.pop()
            if not pending:
                del finalizers[key]
            if owner is None:
                observations.append(dict(kind='missing_pool_generation', seq=seq, pool=hex(r['pool'])))
                continue
            owner['final'] = seq
            # A read already in progress when finalization returns is a separate interval risk.
            for read in reads.values():
                tex = read['texture']
                if read['detail'] != 1 and tex and tex['owner'] is owner and overlaps(read['addr'], read['size'], owner['addr'], owner['size']):
                    note('read_outlives_pool_finalize', read, seq, 'pool finalization end recorded while borrowed CPU read remains open')
        elif e == READ_BEGIN:
            tex = textures.get(host)
            read = dict(r, texture=tex, snapshot=tex['snapshot'] if tex and r['detail'] == 1 else None)
            reads[seq] = read
            read_modes[{0: 'borrowed', 1: 'owned_snapshot', 2: 'snapshot_capture'}.get(r['detail'], 'unknown')] += 1
            read_threads[r['thread']] += 1
            for release in releases.values():
                if overlaps(r['addr'], r['size'], release['addr'], release['size']):
                    release['reads'].append(read)
            if not tex:
                observations.append(dict(kind='unregistered_texture_read', seq=seq, texture=hex(host)))
                continue
            if r['detail'] == 1:
                snap = tex['snapshot']
                if not snap or r['addr'] < snap['addr'] or r['addr'] + r['size'] > snap['addr'] + snap['size']:
                    note('owned_read_outside_recorded_snapshot', read, tex['seq'], 'actual read interval is not inside the registered snapshot')
                elif snap['released']:
                    note('snapshot_read_after_release', read, snap['released'], 'snapshot cleanup event precedes this owned read')
                else:
                    packaged = tex.get('packaged', (0, 0))
                    if any(overlaps(*packaged, release['addr'], release['size']) for release in tex['invalidations']):
                        snapshot_use['reads_after_original_source_release'] += 1
                    if tex['owner'] and tex['owner']['final']:
                        snapshot_use['reads_after_pool_finalize'] += 1
                continue
            owner = tex['owner']
            if owner and overlaps(r['addr'], r['size'], owner['addr'], owner['size']):
                if owner['final']:
                    note('borrowed_read_after_pool_finalize', read, owner['final'], 'pool finalization end precedes borrowed read')
                current = pools.get(tex['pool'])
                if current is not owner:
                    note('borrowed_read_after_pool_reuse', read, current['seq'] if current else 0,
                         'texture retains an older generation of the same pool object address')
            for release in tex['invalidations']:
                if overlaps(r['addr'], r['size'], release['addr'], release['size']):
                    note('borrowed_read_after_memory_release', read, release['end'],
                         'successful free/clear end precedes this read; a later same-address allocation does not renew the old texture')
        elif e == READ_END:
            read = reads.pop(r['detail'], None)
            if not read or read['host'] != host or read['thread'] != r['thread']:
                raise ValueError(f'unmatched CPU read end at sequence {seq}')
        elif e == RELEASE_BEGIN:
            releases[seq] = dict(r, reads=[read for read in reads.values()
                if overlaps(read['addr'], read['size'], r['addr'], r['size'])])
        elif e == RELEASE_END:
            release = releases.pop(host, None)
            if not release:
                raise ValueError(f'unmatched memory release end at sequence {seq}')
            if r['detail']:
                release_kinds[{1: 'decommit', 2: 'release', 3: 'clear'}.get(release['detail'], 'unknown')] += 1
                release['end'] = seq
                if not release['size']:
                    observations.append(dict(kind='unknown_release_extent', seq=release['seq'], address=hex(release['addr'])))
                seen = set()
                for read in release['reads']:
                    if read['seq'] not in seen:
                        note('memory_release_overlaps_cpu_read', read, seq,
                             'successful release/clear interval overlaps recorded read interval; exact fault ordering still requires the crash context')
                        seen.add(read['seq'])
                for candidate in sources.candidates(release['addr'], release['size']):
                    tex = textures[candidate]
                    ranges = [(tex['addr'], tex['size']), tex.get('packaged', (0, 0))]
                    if any(overlaps(a, n, release['addr'], release['size']) for a, n in ranges):
                        tex['invalidations'].append({k: release[k] for k in ('addr', 'size', 'end')})
    return dict(valid_records=count, partial_tail_bytes=partial, open_reads=len(reads),
                open_releases=len(releases), open_pool_finalizers=sum(map(len, finalizers.values())), findings=findings, observations=observations,
                coverage=dict(events=dict(events), reads_by_source=dict(read_modes),
                              read_threads=len(read_threads), texture_formats=dict(texture_formats),
                              pool_generations=events[POOL_INIT], unique_pool_addresses=len(pools),
                              pool_address_reinitializations=pool_reuses,
                              successful_memory_operations=dict(release_kinds),
                              valid_snapshot_reads_after_source_release=snapshot_use['reads_after_original_source_release'],
                              valid_snapshot_reads_after_pool_finalize=snapshot_use['reads_after_pool_finalize']),
                limits=['event prefix only; absent events do not prove absence of a release',
                        'pool finalization/reuse is not a page-free record',
                        'intersecting operation intervals establish a lifecycle risk, not the exact faulting instruction',
                        'allocator reuse without an observed pool reinitialization or page event is not tracked',
                        'linear texture upload/read callbacks are not fully instrumented'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('trace', type=Path)
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    result = analyze(args.trace)
    text = json.dumps(result, ensure_ascii=False, indent=2) + '\n'
    if args.output:
        args.output.write_text(text, encoding='utf-8')
    else:
        print(text, end='')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
