"""Pinned ENet/Zstandard static dependencies for the Yuzu backend (Windows).

Sources are cached under deps/, never copied into generated game source. Rebuilds
only changed objects; linking introduces no extra runtime DLL. curl preserves TLS
verification. Archive hashes make mutable tags or an interrupted download fail closed.
"""
from pathlib import Path
import concurrent.futures
import hashlib
import os
import subprocess
import tarfile

PACKAGES = (
    ('enet', '1.3.18', 'lsalzman/enet', '28603c895f9ed24a846478180ee72c7376b39b4bb1287b73877e5eae7d96b0dd'),
    ('zstd', '1.5.6', 'facebook/zstd', '30f35f71c1203369dc979ecde0400ffea93c27391bfd2ac5a9715d2173d92ff7'),
)


def prepare(clang, workspace):
    root = Path(os.environ.get('SSBU_NETWORK_DEPS', Path(workspace).resolve().parents[1] / 'deps')).resolve()
    downloads = root / 'network-downloads'
    downloads.mkdir(parents=True, exist_ok=True)
    sources = {}
    for name, version, repo, digest in PACKAGES:
        archive = downloads / f'{name}-{version}.tar.gz'
        directory = root / f'{name}-{version}'
        stamp = directory / '.ssbu-source-sha256'
        if not stamp.exists() or stamp.read_text().strip() != digest:
            if not archive.exists():
                temp = archive.with_suffix('.download')
                subprocess.run(['curl.exe', '-fSL', '--retry', '2', '--max-time', '120',
                                f'https://codeload.github.com/{repo}/tar.gz/refs/tags/v{version}',
                                '-o', str(temp)], check=True)
                if hashlib.sha256(temp.read_bytes()).hexdigest() != digest:
                    raise RuntimeError(f'{name}: source archive hash mismatch')
                temp.replace(archive)
            if hashlib.sha256(archive.read_bytes()).hexdigest() != digest:
                raise RuntimeError(f'{name}: source archive hash mismatch: {archive}')
            with tarfile.open(archive, 'r:gz') as tf:
                regular = []
                for member in tf.getmembers():
                    if member.issym() or member.islnk():
                        continue  # CLI convenience links are not needed by either C library.
                    target = (root / member.name).resolve()
                    if not target.is_relative_to(directory) or not (member.isfile() or member.isdir()):
                        raise RuntimeError(f'Unexpected archive entry: {member.name}')
                    regular.append(member)
                tf.extractall(root, members=regular)
            stamp.write_text(digest + '\n')
        sources[name] = directory
    includes = ['-I', str(sources['enet'] / 'include'), '-I', str(sources['zstd'] / 'lib')]
    enet = [p for p in sources['enet'].glob('*.c') if p.name != 'unix.c']
    zstd = [p for sub in ('common', 'compress', 'decompress') for p in (sources['zstd'] / 'lib' / sub).glob('*.c')]
    objdir = Path(workspace) / 'obj_host' / 'network_deps'
    objdir.mkdir(parents=True, exist_ok=True)
    flags = ['-O2', '-w', '-DWIN32', '-DZSTD_DISABLE_ASM'] + includes
    signature = hashlib.sha256((str(Path(clang).resolve()) + repr(flags) + repr(PACKAGES)).encode()).hexdigest()
    stamp = objdir / 'build.signature'
    rebuild = not stamp.exists() or stamp.read_text() != signature
    objects, jobs = [], []
    for src in enet + zstd:
        obj = objdir / (('enet_' if src in enet else 'zstd_') + src.stem + '.obj')
        objects.append(str(obj))
        if rebuild or not obj.exists() or obj.stat().st_mtime < src.stat().st_mtime:
            jobs.append((src, obj))

    def compile_one(job):
        src, obj = job
        p = subprocess.run([clang, '-c', *flags, str(src), '-o', str(obj)], capture_output=True, text=True)
        if p.returncode:
            raise RuntimeError(f'{src}:\n{p.stdout}{p.stderr}')

    with concurrent.futures.ThreadPoolExecutor(max_workers=min(8, os.cpu_count() or 1)) as pool:
        list(pool.map(compile_one, jobs))
    stamp.write_text(signature)
    print(f'network dependencies: ENet 1.3.18 + Zstandard 1.5.6 ({len(jobs)} objects rebuilt)', flush=True)
    return includes, objects
