'use strict';
/* SSBU Console UI. Talks to the Rust side through Tauri commands. Data from files and the game is only ever put into
   the page as text (textContent), never as HTML; the one innerHTML below is the constant controller drawing. */
const T = window.__TAURI__;
const invoke = (cmd, args) => T.core.invoke(cmd, args || {});
const listen = (ev, fn) => T.event.listen(ev, e => fn(e.payload));
const $ = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];
function el(tag, props, ...kids) {
  const n = document.createElement(tag);
  for (const [k, v] of Object.entries(props || {})) {
    if (v == null || v === false) continue;
    if (k === 'class') n.className = v;
    else if (k === 'text') n.textContent = v;
    else if (k.startsWith('on')) n.addEventListener(k.slice(2), v);
    else if (k in n && typeof v !== 'string') n[k] = v;
    else n.setAttribute(k, v === true ? '' : v);
  }
  for (const c of kids.flat()) if (c != null && c !== false) n.append(c.nodeType ? c : String(c));
  return n;
}
let toastTimer;
function toast(msg, err) {
  const t = $('#toast');
  t.textContent = msg; t.className = 'toast' + (err ? ' err' : ''); t.hidden = false;
  clearTimeout(toastTimer); toastTimer = setTimeout(() => (t.hidden = true), err ? 9000 : 4500);
}
const fail = e => toast(String(e), true);
const reveal = p => invoke('reveal', { path: p }).catch(fail);
const zipOk = z => z && !String(z).startsWith('(');

/* the "saved" pill in the corner: every setting saves itself */
let pillTimer;
function pill(text, kind) {
  const p = $('#savePill');
  p.textContent = text; p.dataset.k = kind; p.classList.add('on');
  clearTimeout(pillTimer);
  if (kind !== 'err') pillTimer = setTimeout(() => p.classList.remove('on'), 2200);
}
const savedText = live => st.game.state === 'running' ? live : '已保存';
function debounce(fn, ms) { let t; const d = (...a) => { clearTimeout(t); t = setTimeout(() => fn(...a), ms); }; d.flush = () => { clearTimeout(t); return fn(); }; return d; }

const st = { ov: null, game: { state: 'idle' }, launcher: null, logSeq: 0, logLines: [], net: null, page: 'start',
  pads: null, detected: [], live: {}, prevSrc: {}, padTool: null, padToolT0: 0, target: 'kb', sel: null, listen: null };

const RESULT = { ok: '正常', pass: '通过', warning: '警告', error: '出错', fail: '未通过', not_started: '未启动', running: '运行中' };
const MODE = { play: '游戏', smoke: '冒烟测试', online: '联机', replay: '回放' };
const MARKER = { crash: '宿主崩溃', guest_trap: '游戏代码陷阱', untranslated: '未翻译的指令', device_lost: '显卡设备丢失',
  freeze: '卡死报告', freeze_recovered: '卡死后恢复', debug_end: '指定的战斗时长已到', movie: '片头动画已开始',
  time_limit: '运行时长已到', vulkan_fatal: 'Vulkan 致命错误', nro_refused: '脚本模块被拒绝', rom_missing: '游戏数据文件缺失',
  manual_dump: '手动写过 dump' };
const fmtDur = s => s == null ? '' : s < 90 ? `${Math.round(s)} 秒` : `${Math.round(s / 60)} 分钟`;
const whenOf = name => { const m = /^(\d{4})(\d{2})(\d{2})-(\d{2})(\d{2})/.exec(name || ''); return m ? `${+m[2]}/${+m[3]} ${m[4]}:${m[5]}` : ''; };
const badge = r => el('span', { class: 'badge ' + (r || ''), text: RESULT[r] || r || '无结果' });

/* ================================================================ navigation */
function show(page) {
  if (page === st.page) return;
  const from = st.page;
  st.page = page;
  document.body.dataset.page = page;
  $$('.tile').forEach(b => { if (b.dataset.page === page) b.setAttribute('aria-current', 'page'); else b.removeAttribute('aria-current'); });
  $$('.page').forEach(p => { p.hidden = p.id !== 'page-' + page; p.classList.remove('enter'); });
  const w = $('.wipe'); w.classList.remove('go'); void w.offsetWidth; w.classList.add('go');
  const pg = $('#page-' + page); void pg.offsetWidth; pg.classList.add('enter');
  if (page === 'mods') loadMods();
  if (page === 'graphics') loadGraphics();
  if (page === 'pads') loadPads().then(startPadTest);
  if (page === 'debug') { loadLauncherOpts(); renderLog(); }
  if (page === 'online') loadNetForm();
  if (from === 'pads') { padSave.flush(); if (st.padTool === 'test') stopPadTool(); }
}
$$('.tile').forEach(b => b.addEventListener('click', () => show(b.dataset.page)));

/* ================================================================ start page */
async function refreshOverview() {
  try { st.ov = await invoke('overview'); } catch (e) { return fail(e); }
  const ov = st.ov;
  const fb = $('#fatalBox');
  if (!ov.found) {
    fb.hidden = false; fb.replaceChildren(el('b', { text: '找不到游戏' }), el('span', { text: '控制台要放在包的根文件夹里，和 game、config 文件夹并列。' }));
    $('#btnLaunch').disabled = true;
    return;
  }
  fb.hidden = !ov.preflight.fatal.length;
  fb.replaceChildren(el('b', { text: '还不能启动' }), ...ov.preflight.fatal.map(f => el('span', { text: f })));
  const checks = $('#checks'); checks.replaceChildren();
  let warn = 0;
  for (const f of ov.preflight.fatal) checks.append(el('li', { class: 'fatal', text: f }));
  for (const l of ov.preflight.lines) {
    if (/^package: (MISSING|SIZE|CONTENT)/.test(l)) continue;
    const w = /^WARNING/.test(l); warn += w;
    checks.append(el('li', { class: w ? 'warn' : '', text: l.replace(/^WARNING: /, '') }));
  }
  const nf = ov.preflight.fatal.length;
  $('#drawerSum').textContent = nf ? `${nf} 项问题阻止启动` : warn ? `检查通过，${warn} 项提醒` : '检查全部通过';
  $('#drawer').classList.toggle('alert', !!nf);
  const meta = $('#launchMeta'); meta.replaceChildren();
  const add = (k, v) => v && meta.append(el('dt', { text: k }), el('dd', { text: v }));
  add('build', ov.build_id); add('位置', ov.package ? ov.root : ov.root + '（开发目录）');
  renderSessions(ov.sessions);
  applyGame(ov.game);
}

const MODE_NOTE = {
  play: '启动后可按需到联机页连接服务器。关闭游戏窗口后，日志和报告会自动整理好。',
  smoke: '游戏自己运行一段时间，检查能否正常启动。期间请不要操作。',
};
const curMode = () => $('#modes input:checked').value;
function syncMode() {
  const m = curMode();
  $('#modeNote').textContent = MODE_NOTE[m] + (m === 'smoke' && st.launcher ? `（${st.launcher.smoke_seconds} 秒）` : '');
  if (st.game.state !== 'running') applyGame(st.game);
}
$('#modes').addEventListener('change', syncMode);

function applyGame(g) {
  const prev = st.game.state;
  st.game = g || { state: 'idle' };
  const running = st.game.state === 'running';
  const fatal = !st.ov || !st.ov.found || st.ov.preflight.fatal.length > 0;
  const nameIssue = syncPlayerName(running);
  const blocked = fatal || !!nameIssue;
  const rs = $('#railState');
  rs.dataset.s = running ? 'running' : 'idle';
  $('.txt', rs).textContent = running ? `${MODE[st.game.mode] || ''}运行中` : '未运行';
  const lb = $('#btnLaunch');
  lb.dataset.s = running ? 'running' : blocked ? 'blocked' : 'ready';
  lb.disabled = running || blocked;
  if (running) {
    $('#launchWord').textContent = '运行中';
    $('#launchSub').textContent = `${MODE[st.game.mode] || ''} · ${fmtDur(st.game.uptime_s) || '刚刚启动'}`;
  } else {
    const m = curMode();
    $('#launchWord').textContent = m === 'smoke' ? '冒烟测试' : '开始游戏';
    $('#launchSub').textContent = fatal ? '先解决上面的问题' : nameIssue ? '先填写有效的游戏内名字' : '';
  }
  $('#modes').hidden = running;
  $('#modes').classList.toggle('off', fatal);
  $$('#modes input').forEach(i => (i.disabled = running || fatal));
  $('#modeNote').hidden = running || fatal;
  $('#runActions').hidden = !running;
  $('#btnGoOnline').hidden = !running || st.game.mode === 'smoke';
  $('#btnGoOnline').textContent = st.net && st.net.state === 3 ? '查看联机' : '连接服务器';
  updateNetSteps();
  if (prev === 'running' && !running) refreshOverview();
  if (st.page === 'mods' && prev !== st.game.state) renderMods();
  if (st.page === 'pads') padNote();
}

function renderSessions(list) {
  const ol = $('#sessions'); ol.replaceChildren();
  for (const s of list || []) {
    ol.append(el('li', {}, s.running ? badge('running') : badge(s.result),
      el('span', { class: 'what' }, el('b', { text: MODE[s.mode] || s.mode || '' }), el('small', { text: [whenOf(s.name), fmtDur(s.duration_s)].filter(Boolean).join('，') })),
      el('span', { class: 'links' },
        el('button', { class: 'linkbtn', text: '文件夹', onclick: () => reveal(s.dir) }),
        zipOk(s.zip) ? el('button', { class: 'linkbtn', text: '报告', onclick: () => reveal(s.zip) }) : null)));
  }
  if (!ol.children.length) ol.append(el('li', { class: 'empty', text: '还没有运行记录。' }));
}

function renderLastRun(sum) {
  const box = $('#lastRun'); box.hidden = false; box.replaceChildren();
  box.dataset.r = sum.result;
  const markers = Object.entries(sum.markers || {}).map(([k, v]) => el('li', {}, el('b', { text: MARKER[k] || k }), String(v)));
  if (sum.dumps && sum.dumps.length) markers.push(el('li', {}, el('b', { text: 'minidump' }), sum.dumps.join(', ')));
  box.append(
    el('div', { class: 'stamp', text: RESULT[sum.result] || sum.result || '结束' }),
    el('div', { class: 'lr-body' },
      el('p', { class: 'lr-line', text: `上一次：${MODE[sum.mode] || sum.mode}，${fmtDur(sum.duration_s)}。退出码 ${sum.exit_code}，${sum.exit_meaning}` }),
      markers.length ? el('ul', { class: 'markers' }, markers) : null,
      el('div', { class: 'lr-acts' },
        zipOk(sum.zip) ? el('button', { class: 'linkbtn', text: '显示报告', onclick: () => reveal(sum.zip) }) : null,
        el('button', { class: 'linkbtn', text: '写一句说明', onclick: () => askNote(sum, false) }))));
}

function askNote(sum, auto) {
  const d = $('#dlgNote');
  $('#noteTitle').textContent = auto ? (sum.result === 'warning' ? '这次运行有警告' : '这次运行出错了') : '给这次运行写一句说明';
  $('#noteBody').textContent = auto ? `${sum.exit_meaning}。说明会和日志一起放进报告 zip，方便开发者复现。` : '说明会写进 note.txt，并重新打包报告 zip。';
  $('#noteText').value = '';
  d.onclose = async () => {
    const text = $('#noteText').value.trim();
    if (d.returnValue === 'save' && text) {
      try { await invoke('session_note', { session: sum.session, text }); toast('已写进报告。'); } catch (e) { fail(e); }
    }
    if (auto && zipOk(sum.zip)) reveal(sum.zip);
  };
  d.showModal();
}

/* One identity for local play, online play and smoke tests. */
let playerNameLoaded = false, playerNameSaved = null, playerNameQueue = Promise.resolve();
function playerNameError() {
  if (!playerNameLoaded) return '正在读取游戏内名字…';
  const value = $('#gameName').value.trim();
  if (!value) return '请先填写游戏内名字，再启动游戏。';
  if (new TextEncoder().encode(value).length > 32) return '名字超过 32 字节，请缩短后再启动。';
  if (/[\u0000-\u001f\u007f-\u009f]/.test(value)) return '名字不能包含换行或控制字符。';
  return '';
}
function syncPlayerName(running) {
  const input = $('#gameName'), value = input.value.trim(), error = playerNameError();
  input.disabled = running || !playerNameLoaded;
  input.setCustomValidity(error);
  $('#startProfile').dataset.invalid = String(!!error && playerNameLoaded);
  $('#gameNameCount').textContent = `${new TextEncoder().encode(value).length} / 32 字节`;
  $('#gameNameHint').textContent = running ? '本次游戏正在使用已设置的名字，关闭游戏后可修改。' : error || '支持中文。所有启动方式都会使用这个名字。';
  return error;
}
function savePlayerName() {
  if (!playerNameLoaded) return Promise.resolve();
  const value = $('#gameName').value.trim();
  if (value && playerNameError()) return Promise.reject(new Error(playerNameError()));
  playerNameQueue = playerNameQueue.catch(() => {}).then(async () => {
    if (playerNameSaved === value) return;
    st.launcher = await invoke('launcher_set', {value:{game_name:value}});
    playerNameSaved = value;
    pill(value ? '名字已保存' : '已清空，填写名字后才能启动', 'ok');
  });
  return playerNameQueue;
}
const playerNameSave = debounce(() => savePlayerName().catch(e => pill(String(e), 'err')), 400);
$('#gameName').addEventListener('input', () => { applyGame(st.game); if (!playerNameError() || !$('#gameName').value.trim()) playerNameSave(); });
$('#gameName').addEventListener('change', () => { savePlayerName().catch(e => pill(String(e), 'err')); });
$('#gameName').addEventListener('keydown', e => { if (e.key === 'Enter') { e.preventDefault(); if (!$('#btnLaunch').disabled) $('#btnLaunch').click(); } });
$('#netGoName').addEventListener('click', () => { show('start'); $('#gameName').focus(); });

async function startGame(mode) {
  if (playerNameError()) {
    show('start'); $('#gameName').focus(); $('#gameName').reportValidity(); return false;
  }
  const run = mode === 'smoke' ? {} : collectRunOpts();
  try {
    await savePlayerName();
    if (st.padTool) await stopPadTool();
    const r = await invoke('game_start', { mode, env: run.env || [], seconds: run.seconds ?? null });
    st.logSeq = 0; st.logLines = []; renderLog();
    toast(mode === 'smoke' ? '冒烟测试开始：请不要操作，游戏会自己运行。' : '游戏已启动。');
    applyGame({ state: 'running', mode, session: r.session, pid: r.pid, uptime_s: 0 });
    return true;
  } catch (e) { fail(e); refreshOverview(); return false; }
}
$('#btnLaunch').addEventListener('click', async () => {
  const b = $('#btnLaunch');
  b.classList.remove('fire'); void b.offsetWidth; b.classList.add('fire');
  startGame(curMode());
});
$('#btnGoOnline').addEventListener('click', () => show('online'));
$('#btnGoLog').addEventListener('click', () => show('debug'));
$('#btnKill').addEventListener('click', async () => {
  if (!confirm('强制结束游戏？未保存的进度会丢失。正常情况下请直接关闭游戏窗口。')) return;
  invoke('game_kill').catch(fail);
});

listen('game-ended', sum => {
  renderLastRun(sum);
  applyGame({ state: 'ended', finished: sum });
  st.net = null; renderNet(null);
  const bad = ['error', 'fail', 'warning'].includes(sum.result);
  if (bad && sum.mode !== 'smoke') askNote(sum, true);
  else toast(`运行结束：${RESULT[sum.result] || sum.result}。${zipOk(sum.zip) ? '报告已生成。' : ''}`, bad);
  if (st.page === 'pads') startPadTest();
});

async function loadDataPaths() {
  try {
    const v = await invoke('launcher_get'); st.launcher = v;
    $('#romfs').value = v.romfs; $('#userdata').value = v.userdata;
    $('#gameName').value = v.game_name || ''; playerNameSaved = v.game_name || ''; playerNameLoaded = true;
    applyGame(st.game);
    $('#romfsDefault').textContent = '留空时使用 ' + v.romfs_default;
    syncMode();
  } catch (e) { fail(e); }
}
for (const id of ['#romfs', '#userdata']) $(id).addEventListener('change', async () => {
  try {
    st.launcher = await invoke('launcher_set', { value: { romfs: $('#romfs').value.trim(), userdata: $('#userdata').value.trim() } });
    pill('已保存', 'ok'); refreshOverview();
  } catch (e) { pill(String(e), 'err'); }
});

/* the start page's background picture: config\cover.* (cover.rs), darkened by the page */
function paintCover(c) {
  const url = c && c.url;
  for (const n of [$('#cover'), $('#coverThumb')]) n.style.backgroundImage = url ? `url("${url}")` : '';
  $('#page-start').classList.toggle('has-cover', !!url);
  $('#coverThumb').classList.toggle('empty', !url);
  $('#coverClear').hidden = !url;
  $('#coverPick').textContent = url ? '换一张' : '选择图片';
  if (c && c.error) toast(c.error, true);
}
const loadCover = () => invoke('cover_get').then(paintCover).catch(fail);
$('#coverPick').addEventListener('click', () => $('#coverFile').click());
$('#coverFile').addEventListener('change', e => {
  const f = e.target.files[0];
  e.target.value = '';
  if (!f) return;
  if (f.size > 24 << 20) return toast('图片太大（最多 24 MB）。', true);
  const r = new FileReader();
  r.onload = async () => {
    try { paintCover(await invoke('cover_set', { data: String(r.result).replace(/^data:[^,]*,/, '') })); pill('背景已更换', 'ok'); }
    catch (err) { fail(err); }
  };
  r.readAsDataURL(f);
});
$('#coverClear').addEventListener('click', async () => {
  try { paintCover(await invoke('cover_set', { data: null })); pill('背景已移除', 'ok'); } catch (err) { fail(err); }
});

/* ================================================================ graphics */
const gfx = $('#gfxForm');
const GFX_KEYS = ['renderer', 'window', 'vsync', 'latency', 'profile'];
function setScale(v) {
  v = Math.min(2, Math.max(0.25, +v || 1));
  $('#scale').value = v;
  $('#scale').style.setProperty('--p', ((v - 0.25) / 1.75 * 100).toFixed(1) + '%');
  $('#scaleOut').replaceChildren(el('b', { text: (+v).toFixed(2) }), el('i', { text: '×' }), el('small', { text: `${Math.round(1920 * v)} × ${Math.round(1080 * v)}` }));
  $$('#scalePresets button').forEach(b => b.setAttribute('aria-pressed', Math.abs(+b.dataset.v - v) < 0.001));
}
function syncProfile() {
  const low = gfx.querySelector('input[name="profile"]:checked')?.value === 'low';
  for (const id of ['#rowScale', '#rowVsync']) { $(id).classList.toggle('dim', low); $('.locked', $(id)).hidden = !low; }
}
async function loadGraphics() {
  try {
    const g = await invoke('graphics_get');
    for (const k of GFX_KEYS) {
      const r = gfx.querySelector(`input[name="${k}"][value="${g.values[k]}"]`) || gfx.querySelector(`input[name="${k}"]`);
      r.checked = true;
    }
    setScale(g.values.resolution_scale);
    syncProfile();
    $('#gfxPath').textContent = g.path + (g.exists ? '' : '（还不存在，第一次改动时创建）');
  } catch (e) { fail(e); }
}
async function saveGraphics() {
  const values = {};
  for (const k of GFX_KEYS) values[k] = gfx.querySelector(`input[name="${k}"]:checked`).value;
  values.resolution_scale = (+$('#scale').value).toFixed(2).replace(/0$/, '');
  try { await invoke('graphics_set', { values }); pill(savedText('已保存，下次启动游戏时生效'), 'ok'); }
  catch (err) { pill(String(err), 'err'); }
}
$('#scale').addEventListener('input', e => setScale(e.target.value));
$('#scale').addEventListener('change', saveGraphics);
$$('#scalePresets button').forEach(b => b.addEventListener('click', () => { setScale(b.dataset.v); saveGraphics(); }));
gfx.addEventListener('change', e => { if (e.target.type !== 'radio') return; if (e.target.name === 'profile') syncProfile(); saveGraphics(); });
gfx.addEventListener('submit', e => e.preventDefault());

/* ================================================================ controllers */
const PRO = ['A', 'B', 'X', 'Y', 'L', 'R', 'ZL', 'ZR', 'PLUS', 'MINUS', 'LEFT', 'UP', 'RIGHT', 'DOWN', 'STICK_L', 'STICK_R'];
const TITLE = { A: 'A', B: 'B', X: 'X', Y: 'Y', L: 'L', R: 'R', ZL: 'ZL', ZR: 'ZR', PLUS: '+', MINUS: '−', UP: '十字上', DOWN: '十字下',
  LEFT: '十字左', RIGHT: '十字右', STICK_L: '左摇杆按下', STICK_R: '右摇杆按下', LSTICK: '左摇杆', RSTICK: '右摇杆' };
const DIRS = ['UP', 'LEFT', 'DOWN', 'RIGHT'];
const DIR_ARROW = { UP: '↑', LEFT: '←', DOWN: '↓', RIGHT: '→' };
const keyLabel = key => ({ ...DIR_ARROW, BACKSPACE: '退格', ENTER: 'Enter', SPACE: '空格' })[key.toUpperCase()] || key;
const KB_DEFAULT = { A: 'Z', B: 'X', X: 'C', Y: 'V', L: 'Q', R: 'E', ZL: 'A', ZR: 'S', PLUS: 'ENTER', MINUS: 'BACKSPACE', LEFT: 'J', UP: 'I',
  RIGHT: 'L', DOWN: 'K', STICK_L: 'none', STICK_R: 'none', LSTICK_LEFT: 'LEFT', LSTICK_UP: 'UP', LSTICK_RIGHT: 'RIGHT', LSTICK_DOWN: 'DOWN',
  RSTICK_LEFT: 'none', RSTICK_UP: 'none', RSTICK_RIGHT: 'none', RSTICK_DOWN: 'none' };
/* the game's built-in mapping of an SDL gamepad with the positional layout (nnpad_sdl.c def_map) */
const DEF_SRC = { A: 'east', B: 'south', X: 'north', Y: 'west', L: 'leftshoulder', R: 'rightshoulder', ZL: 'lefttrigger', ZR: 'righttrigger',
  PLUS: 'start', MINUS: 'back', LEFT: 'dpleft', UP: 'dpup', RIGHT: 'dpright', DOWN: 'dpdown', STICK_L: 'leftstick', STICK_R: 'rightstick' };
const SRC_LABEL = { south: '下面键', east: '右面键', west: '左面键', north: '上面键', back: 'Back / Select', guide: 'Home', start: 'Start',
  leftstick: '左摇杆按下', rightstick: '右摇杆按下', leftshoulder: '左肩键', rightshoulder: '右肩键', dpup: '十字上', dpdown: '十字下',
  dpleft: '十字左', dpright: '十字右', misc1: '截图键', rightpaddle1: '右背键 1', leftpaddle1: '左背键 1', rightpaddle2: '右背键 2',
  leftpaddle2: '左背键 2', touchpad: '触摸板', 'leftx+': '左摇杆推右', 'leftx-': '左摇杆推左', 'lefty+': '左摇杆推下', 'lefty-': '左摇杆推上',
  'rightx+': '右摇杆推右', 'rightx-': '右摇杆推左', 'righty+': '右摇杆推下', 'righty-': '右摇杆推上', lefttrigger: '左扳机', righttrigger: '右扳机',
  leftx: '左摇杆 X', lefty: '左摇杆 Y', rightx: '右摇杆 X', righty: '右摇杆 Y', none: '不映射' };
const SOURCES = Object.keys(SRC_LABEL).filter(s => !/^(left|right)[xy]$/.test(s));
const AXES = ['leftx', 'lefty', 'rightx', 'righty', 'axis0', 'axis1', 'axis2', 'axis3', 'axis4', 'axis5'];
function srcLabel(s) {
  s = s.trim().toLowerCase();
  if (SRC_LABEL[s]) return SRC_LABEL[s];
  let m;
  if ((m = /^button(\d+)$/.exec(s))) return `按钮 ${m[1]}`;
  if ((m = /^axis(\d+)([+-]?)$/.exec(s))) return `轴 ${m[1]}${m[2]}`;
  if ((m = /^hat(\d+)(up|down|left|right)$/.exec(s))) return `方向帽 ${m[1]} ${{ up: '上', down: '下', left: '左', right: '右' }[m[2]]}`;
  return s;
}
document.body.append(el('datalist', { id: 'srcList' }, SOURCES.map(s => el('option', { value: s, label: SRC_LABEL[s] }))),
  el('datalist', { id: 'axList' }, AXES.map(s => el('option', { value: s }))));

$$('#page-pads .subtabs button').forEach(b => b.addEventListener('click', () => {
  $$('#page-pads .subtabs button').forEach(x => x.setAttribute('aria-selected', x === b));
  $$('#page-pads .sub').forEach(s => (s.hidden = s.id !== 'sub-' + b.dataset.sub));
}));

/* the controller drawing: every control is a <g data-b>, its painted surfaces are .k; the list beside it is grouped by area */
const PAD_SVG = `<svg viewBox="12 36 576 370" class="padsvg" role="group" aria-label="Pro 手柄，点一个按键修改它的映射">
  <defs><linearGradient id="padSurface" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="var(--pad-top)"/><stop offset="1" stop-color="var(--pad-bottom)"/></linearGradient></defs>
  <g class="ctl shoulder back" data-b="ZL" tabindex="0" role="button" aria-label="ZL"><path class="k" d="M104 96 C114 64 140 46 178 46 L238 46 Q250 46 250 58 L250 90 Z"/><text x="196" y="63">ZL</text></g>
  <g class="ctl shoulder back" data-b="ZR" tabindex="0" role="button" aria-label="ZR"><path class="k" d="M104 96 C114 64 140 46 178 46 L238 46 Q250 46 250 58 L250 90 Z" transform="matrix(-1 0 0 1 600 0)"/><text x="404" y="63">ZR</text></g>
  <g class="ctl shoulder" data-b="L" tabindex="0" role="button" aria-label="L"><path class="k" d="M84 132 C96 92 128 70 172 70 L244 70 Q254 70 254 80 L254 98 Z"/><text x="206" y="86">L</text></g>
  <g class="ctl shoulder" data-b="R" tabindex="0" role="button" aria-label="R"><path class="k" d="M84 132 C96 92 128 70 172 70 L244 70 Q254 70 254 80 L254 98 Z" transform="matrix(-1 0 0 1 600 0)"/><text x="394" y="86">R</text></g>
  <path class="body" d="M300 286 C220 286 180 290 150 312 C130 330 108 372 88 384 C60 400 22 380 28 330 C32 290 55 200 72 150 C85 110 120 92 170 92 L430 92 C480 92 515 110 528 150 C545 200 568 290 572 330 C578 380 540 400 512 384 C492 372 470 330 450 312 C420 290 380 286 300 286 Z"/>
  <g class="deco" aria-hidden="true"><rect x="262" y="146" width="14" height="14" rx="3"/><circle cx="331" cy="153" r="8"/><circle cx="331" cy="153" r="3.5"/></g>
  <g class="ctl small" data-b="MINUS" tabindex="0" role="button" aria-label="−"><rect class="k" x="224" y="119" width="24" height="9" rx="3"/></g>
  <g class="ctl small" data-b="PLUS" tabindex="0" role="button" aria-label="+"><path class="k" d="M360 113 h8 v7 h7 v8 h-7 v7 h-8 v-7 h-7 v-8 h7 z"/></g>
  <g class="ctl stick" data-b="LSTICK" tabindex="0" role="button" aria-label="左摇杆"><circle class="well" cx="170" cy="162" r="38"/><g class="cap"><circle class="k" cx="170" cy="162" r="27"/><circle class="ring" cx="170" cy="162" r="18"/></g></g>
  <g class="ctl stick" data-b="RSTICK" tabindex="0" role="button" aria-label="右摇杆"><circle class="well" cx="364" cy="224" r="38"/><g class="cap"><circle class="k" cx="364" cy="224" r="27"/><circle class="ring" cx="364" cy="224" r="18"/></g></g>
  <g class="dpad"><circle class="well" cx="236" cy="224" r="40"/>
    <g class="ctl arm" data-b="UP" tabindex="0" role="button" aria-label="十字上"><path class="k" d="M226 191 h20 v22 l-10 10 l-10 -10 z"/></g>
    <g class="ctl arm" data-b="DOWN" tabindex="0" role="button" aria-label="十字下"><path class="k" d="M226 257 h20 v-22 l-10 -10 l-10 10 z"/></g>
    <g class="ctl arm" data-b="LEFT" tabindex="0" role="button" aria-label="十字左"><path class="k" d="M203 214 v20 h22 l10 -10 l-10 -10 z"/></g>
    <g class="ctl arm" data-b="RIGHT" tabindex="0" role="button" aria-label="十字右"><path class="k" d="M269 214 v20 h-22 l-10 -10 l10 -10 z"/></g>
  </g>
  <g class="ctl face" data-b="X" tabindex="0" role="button" aria-label="X"><circle class="k" cx="432" cy="130" r="17"/><text x="432" y="136">X</text></g>
  <g class="ctl face" data-b="A" tabindex="0" role="button" aria-label="A"><circle class="k" cx="464" cy="162" r="17"/><text x="464" y="168">A</text></g>
  <g class="ctl face" data-b="Y" tabindex="0" role="button" aria-label="Y"><circle class="k" cx="400" cy="162" r="17"/><text x="400" y="168">Y</text></g>
  <g class="ctl face" data-b="B" tabindex="0" role="button" aria-label="B"><circle class="k" cx="432" cy="194" r="17"/><text x="432" y="200">B</text></g>
</svg>`;
const GROUPS = [['面键', ['A', 'B', 'X', 'Y']], ['肩键', ['L', 'R', 'ZL', 'ZR']], ['十字键', ['UP', 'DOWN', 'LEFT', 'RIGHT']],
  ['摇杆', ['LSTICK', 'STICK_L', 'RSTICK', 'STICK_R']], ['加减键', ['PLUS', 'MINUS']]];
const CTL_OF = { STICK_L: 'LSTICK', STICK_R: 'RSTICK' };     /* the stick press lights the stick */
$('#pad').innerHTML = PAD_SVG;
$('#pad').addEventListener('click', e => { const g = e.target.closest('[data-b]'); if (g) select(g.dataset.b); });
$('#pad').addEventListener('keydown', e => { const g = e.target.closest('[data-b]'); if (g && (e.key === 'Enter' || e.key === ' ')) { e.preventDefault(); select(g.dataset.b); } });
/* pointing at a row lights its control on the drawing, pointing at a control lights its rows */
function hot(b) {
  $$('#pad [data-b]').forEach(g => g.classList.toggle('hot', !!b && (g.dataset.b === b || g.dataset.b === CTL_OF[b])));
  $$('#binds li[data-b]').forEach(li => li.classList.toggle('hot', !!b && (li.dataset.b === b || CTL_OF[li.dataset.b] === b)));
}
for (const id of ['#pad', '#binds']) {
  $(id).addEventListener('pointerover', e => hot(e.target.closest('[data-b]')?.dataset.b));
  $(id).addEventListener('pointerleave', () => hot(null));
}

const isKb = () => st.target === 'kb';
const detectedOf = guid => st.detected.find(d => d.guid === guid);
const devCfg = (guid, create) => {
  let d = st.pads.devices.find(x => x.guid === guid);
  if (!d && create) {
    d = { guid, name: (detectedOf(guid) || {}).name || 'pad', map: {}, axis: {}, invert: {} };
    st.pads.devices.push(d);
  }
  return d;
};
function effLayout(guid) {
  const c = devCfg(guid), det = detectedOf(guid);
  const l = (c && c.layout) || st.pads.general.layout;
  return l === 'auto' ? (det && /gamecube/i.test(det.type + det.name) ? 'labels' : 'positional') : l;
}

async function loadPads() {
  try { st.pads = await invoke('controllers_get'); } catch (e) { return fail(e); }
  const p = st.pads, g = p.general, f = $('#padGeneral');
  (f.querySelector(`input[name="layout"][value="${g.layout}"]`) || f.querySelector('input[name="layout"]')).checked = true;
  for (const k of ['deadzone', 'outer_zone', 'trigger_threshold', 'rumble_strength']) { f[k].value = g[k]; sliderOut(f[k]); }
  f.rumble.checked = g.rumble !== 'off';
  zoneViz();
  renderIgnore(); renderSlots(); renderPadAll();
  $('#padPath').textContent = p.path;
}
function renderPadAll() { renderCards(); renderTarget(); renderBinds(); renderEditor(); padNote(); paintLive(); }

function padNote() {
  const n = $('#padNote');
  if (st.game.state === 'running') n.textContent = '游戏运行中：映射照常可以改，但手柄检测和按键录入要等游戏关闭。';
  else if (st.padTool === 'test') n.textContent = st.detected.length ? '' : '没有找到手柄。插上手柄后会自动出现。';
  else n.textContent = '';
}

/* ---- target cards: the keyboard, the pads SDL sees, the pads with settings that are not plugged in */
function renderCards() {
  const box = $('#padCards'); box.replaceChildren();
  const card = (id, name, sub, player, extra) => {
    const b = el('button', { class: 'card' + (extra || ''), role: 'option', 'aria-selected': String(st.target === id), onclick: () => { st.target = id; st.sel = null; st.listen = null; renderPadAll(); } },
      player ? el('span', { class: 'pn', 'data-p': player, text: `${player}P` }) : el('span', { class: 'pn kb', text: '键' }),
      el('span', { class: 'cn' }, el('b', { text: name }), el('small', { text: sub })));
    box.append(b);
  };
  const kbp = st.pads.keyboard.player;
  card('kb', '键盘', kbp === 'none' ? '不使用' : kbp === 'auto' ? '自动分配玩家' : `${kbp}P`, null);
  for (const d of st.detected) card(d.guid, d.name, d.mapped ? d.type : '无映射，需要录入', d.player, d.mapped ? '' : ' warn');
  for (const c of st.pads.devices) if (!detectedOf(c.guid)) card(c.guid, c.name || '未知设备', '未连接', null, ' gone');
  if (st.target !== 'kb' && !detectedOf(st.target) && !devCfg(st.target)) { st.target = 'kb'; renderCards(); }
}

function renderTarget() {
  const box = $('#target'); box.replaceChildren();
  if (isKb()) {
    const sel = el('select', { 'aria-label': '键盘是几号玩家', onchange: e => { st.pads.keyboard.player = e.target.value; padSave(); renderCards(); } },
      ['auto', '1', '2', '3', '4', '5', '6', '7', '8', 'none'].map(v => el('option', { value: v, text: v === 'auto' ? '自动' : v === 'none' ? '不使用键盘' : `${v}P`, selected: st.pads.keyboard.player === v })));
    box.append(el('div', { class: 'tg-row' }, el('label', { class: 'inline' }, '键盘是', sel),
      el('button', { class: 'linkbtn', text: '全部恢复默认', onclick: () => { Object.assign(st.pads.keyboard.keys, KB_DEFAULT); padSave(); renderPadAll(); } })),
      el('details', { class: 'help' }, el('summary', { text: '说明' }),
        el('p', { text: '点手柄图上的按键，再按键盘上的键。自动 = 启动时已连接的手柄之后的第一个空槽位：有一个手柄时手柄是 1P、键盘是 2P；没有手柄时键盘是 1P。指定号码时与该玩家的手柄合并。只有游戏窗口在前台时键盘才有效。' })));
    return;
  }
  const g = st.target, det = detectedOf(g), cfg = devCfg(g);
  const tags = det ? [det.type, !det.mapped && '无映射', det.wireless && '无线', det.gyro && '陀螺仪', det.rumble && '震动', `${det.buttons} 键 ${det.axes} 轴 ${det.hats} 方向`].filter(Boolean) : ['未连接'];
  box.append(el('div', { class: 'tg-row' },
    el('span', { class: 'guid', text: g }), el('span', { class: 'tags' }, tags.map(t => el('span', { class: 'tag', text: t }))), el('span', { class: 'grow' }),
    det && st.game.state !== 'running' ? el('button', { class: 'btn small', text: '逐键录入', onclick: () => runPadMap(det) }) : null,
    el('button', { class: 'linkbtn', text: '不使用这个设备', onclick: () => ignoreDevice(g, (det || cfg || {}).name) }),
    cfg ? el('button', { class: 'linkbtn', text: '删除单独设置', onclick: () => { st.pads.devices = st.pads.devices.filter(x => x.guid !== g); padSave(); renderPadAll(); } }) : null));
  /* per-device overrides of the general values */
  const gen = st.pads.general, c = cfg || {};
  const num = (k, min, max, step) => el('label', { class: 'field' }, el('span', { class: 'lbl', text: { deadzone: '死区', outer_zone: '外圈', trigger_threshold: '扳机阈值', rumble_strength: '震动强度' }[k] }),
    el('input', { type: 'number', min, max, step, value: c[k] || '', placeholder: `同通用 ${gen[k]}`, oninput: e => { devCfg(g, true)[k] = e.target.value.trim(); padSave(); } }));
  const sel = (k, lbl, opts) => el('label', { class: 'field' }, el('span', { class: 'lbl', text: lbl }),
    el('select', { onchange: e => { devCfg(g, true)[k] = e.target.value; padSave(); renderBinds(); } }, opts.map(([v, t]) => el('option', { value: v, text: t, selected: (c[k] || '') === v }))));
  box.append(el('details', { class: 'overrides' }, el('summary', { text: '这个设备的单独参数' }),
    el('div', { class: 'grid3' },
      sel('layout', '面键布局', [['', '同通用'], ['auto', '自动'], ['positional', '按位置'], ['labels', '按标签']]),
      num('deadzone', '0', '0.9', '0.01'), num('outer_zone', '0.1', '1', '0.01'), num('trigger_threshold', '0.01', '1', '0.01'),
      sel('rumble', '震动', [['', '同通用'], ['on', '开'], ['off', '关']]), num('rumble_strength', '0', '2', '0.1')),
    el('p', { class: 'hint', text: '留空的项使用“通用”页的值。' })));
}

/* ---- what each control does for the current target */
function bindingOf(b) {
  if (isKb()) {
    const k = st.pads.keyboard.keys;
    if (b === 'LSTICK' || b === 'RSTICK') return { keys: DIRS.map(d => [d, k[`${b}_${d}`] || 'none']) };
    const v = k[b] || 'none';
    return { main: v.toLowerCase() === 'none' ? '未绑定' : v, none: v.toLowerCase() === 'none' };
  }
  const c = devCfg(st.target) || { map: {}, axis: {}, invert: {} }, det = detectedOf(st.target);
  if (b === 'LSTICK' || b === 'RSTICK') {
    const [x, y] = b === 'LSTICK' ? ['LX', 'LY'] : ['RX', 'RY'];
    if (!c.axis[x] && !c.axis[y] && c.invert[x] !== 'on' && c.invert[y] !== 'on') return { main: det && det.mapped ? (b === 'LSTICK' ? '左摇杆 X / Y' : '右摇杆 X / Y') : '设备默认摇杆', sub: '默认', def: true };
    const inv = [c.invert[x] === 'on' && 'X 反转', c.invert[y] === 'on' && 'Y 反转'].filter(Boolean).join('，');
    return { main: `${c.axis[x] || '默认'} / ${c.axis[y] || '默认'}`, sub: inv };
  }
  const v = (c.map[b] || '').trim();
  if (!v) {
    const known = det && det.mapped && (!['A', 'B', 'X', 'Y'].includes(b) || effLayout(st.target) === 'positional');
    return { main: known ? srcLabel(DEF_SRC[b]) : '设备默认输入', sub: known ? '默认' : '', def: true };
  }
  if (v.toLowerCase() === 'none') return { main: '不映射', none: true };
  const parts = v.split(',').map(s => s.trim()).filter(Boolean);
  return { main: parts.map(srcLabel).join('，'), sub: parts.join(', ') };
}
function renderBinds() {
  const box = $('#binds'); box.replaceChildren();
  for (const [name, list] of GROUPS) {
    const ul = el('ul');
    for (const b of list) {
      const v = bindingOf(b);
      const val = v.keys
        ? el('span', { class: 'keys4' }, v.keys.map(([d, k]) => { const none = k.toLowerCase() === 'none'; return el('kbd', { class: none ? 'none' : '', title: `${DIR_ARROW[d]} ${k}` }, el('i', { text: DIR_ARROW[d] }), none ? '–' : keyLabel(k)); }))
        : isKb() && !v.none ? el('span', { class: 'src' }, el('kbd', { text: keyLabel(v.main), title: v.main }))
        : el('span', { class: 'src' }, el('span', { text: v.main }), v.sub && !v.def ? el('small', { text: v.sub }) : null);
      const source = v.keys ? v.keys.map(([d, k]) => `${DIR_ARROW[d]} ${k}`).join('，') : v.main;
      ul.append(el('li', { 'data-b': b, class: st.sel === b ? 'sel' : '' },
        el('button', { class: 'brow' + (v.def ? ' def' : '') + (v.none ? ' none' : ''), onclick: () => select(b), 'aria-pressed': String(st.sel === b), 'aria-label': `${TITLE[b]}：${source}` },
          el('span', { class: 'glyph g-' + b, text: TITLE[b] }), val, v.def ? el('span', { class: 'deftag', text: '默认' }) : null)));
    }
    box.append(el('section', { class: 'bgroup' }, el('h3', { text: name }), ul));
  }
  $$('#pad [data-b]').forEach(g => {
    const on = st.sel === g.dataset.b || CTL_OF[st.sel] === g.dataset.b;
    g.classList.toggle('sel', on);
    g.setAttribute('aria-pressed', String(on));
  });
}

function select(b) {
  st.sel = st.sel === b ? null : b;
  st.listen = null;
  renderBinds(); renderEditor();
  if (st.sel) $(`#binds li[data-b="${st.sel}"]`).scrollIntoView({ block: 'nearest' });
  if (st.sel && !isKb() && !/STICK$/.test(b) && detectedOf(st.target) && st.padTool === 'test') listenPad(b);
  if (st.sel && isKb() && !/^(L|R)STICK$/.test(b)) listenKey(b);
}

/* ---- the editor under the drawing */
function renderEditor() {
  const ed = $('#editor'), b = st.sel;
  ed.hidden = !b;
  ed.replaceChildren();
  if (!b) return;
  const head = el('div', { class: 'ed-head' }, el('span', { class: 'glyph big g-' + b, text: TITLE[b] }), el('h3', { text: `游戏内 ${TITLE[b]}` }), el('span', { class: 'grow' }),
    el('button', { class: 'linkbtn', text: '关闭', onclick: () => select(b) }));
  ed.append(head);
  if (isKb()) {
    const keys = b === 'LSTICK' || b === 'RSTICK' ? DIRS.map(d => `${b}_${d}`) : [b];
    const row = el('div', { class: 'keyrow' + (keys.length > 1 ? ' cross' : '') });     /* a stick: the four keys laid out like arrow keys */
    for (const k of keys) {
      const v = st.pads.keyboard.keys[k] || 'none', none = v.toLowerCase() === 'none';
      const on = st.listen && st.listen.kind === 'key' && st.listen.key === k;
      const dir = k.split('_')[1];
      row.append(el('div', { class: 'keycell' },
        keys.length > 1 ? el('span', { class: 'lbl', text: `推${{ UP: '上', LEFT: '左', DOWN: '下', RIGHT: '右' }[dir]}` }) : null,
        el('button', { class: 'keycap' + (on ? ' listening' : '') + (none ? ' none' : ''), text: on ? '按一个键' : none ? '未绑定' : v, onclick: () => listenKey(k) }),
        el('span', { class: 'kacts' },
          el('button', { class: 'linkbtn', text: '解除', onclick: () => { st.pads.keyboard.keys[k] = 'none'; st.listen = null; padSave(); renderBinds(); renderEditor(); } }),
          el('button', { class: 'linkbtn', text: `默认 ${KB_DEFAULT[k] === 'none' ? '无' : KB_DEFAULT[k]}`, onclick: () => { st.pads.keyboard.keys[k] = KB_DEFAULT[k]; st.listen = null; padSave(); renderBinds(); renderEditor(); } }))));
    }
    ed.append(row, el('p', { class: 'hint', text: st.listen ? '按键盘上的键绑定，Esc 取消。' : '点键帽，再按键盘上的键。' }));
    return;
  }
  const g = st.target, det = detectedOf(g);
  if (b === 'LSTICK' || b === 'RSTICK') {
    const c = devCfg(g) || { axis: {}, invert: {} };
    const grid = el('div', { class: 'grid2' });
    for (const a of b === 'LSTICK' ? ['LX', 'LY'] : ['RX', 'RY']) {
      grid.append(el('div', { class: 'axcell' },
        el('label', { class: 'field' }, el('span', { class: 'lbl', text: a.endsWith('X') ? '横向（X）' : '纵向（Y）' }),
          el('input', { list: 'axList', value: c.axis[a] || '', placeholder: '默认', spellcheck: 'false', onchange: e => { devCfg(g, true).axis[a] = e.target.value.trim(); padSave(); renderBinds(); } })),
        el('label', { class: 'toggle small' }, el('input', { type: 'checkbox', checked: c.invert[a] === 'on', onchange: e => { devCfg(g, true).invert[a] = e.target.checked ? 'on' : ''; padSave(); renderBinds(); } }), el('span', { text: '反转' }))));
    }
    ed.append(grid, el('p', { class: 'hint', text: '摇杆轴通常不用改。SDL 的 Y 轴向下为正，这已经自动处理，“反转”只在方向确实反了时才勾。' }));
    return;
  }
  const c = devCfg(g), cur = ((c && c.map[b]) || '').trim();
  const parts = cur && cur.toLowerCase() !== 'none' ? cur.split(',').map(s => s.trim()).filter(Boolean) : [];
  const setMap = v => { devCfg(g, true).map[b] = v; st.listen = null; padSave(); renderBinds(); renderEditor(); };
  const on = st.listen && st.listen.kind === 'pad';
  const listenBox = el('div', { class: 'listenbox' + (on ? ' on' : '') },
    det ? (st.padTool === 'test'
      ? el('button', { class: 'btn' + (on ? ' primary' : ''), text: on ? '请按手柄上的键…' : '按手柄绑定', onclick: () => (on ? (st.listen = null, renderEditor()) : listenPad(b)) })
      : el('span', { class: 'hint', text: st.game.state === 'running' ? '游戏运行中，不能用手柄绑定。' : '检测没有在运行。' }))
      : el('span', { class: 'hint', text: '这个设备没有连接，只能手动填写。' }));
  const chips = el('div', { class: 'srcchips' }, parts.length ? parts.map((p, i) => el('span', { class: 'chip' }, el('b', { text: srcLabel(p) }), el('small', { text: p }),
    el('button', { class: 'x', 'aria-label': `移除 ${p}`, text: '×', onclick: () => { parts.splice(i, 1); setMap(parts.join(', ')); } })))
    : el('span', { class: 'hint', text: cur.toLowerCase() === 'none' ? '这个键不起作用。' : `使用默认映射${bindingOf(b).sub ? '：' + bindingOf(b).main : ''}。` }));
  const add = el('input', { list: 'srcList', placeholder: '添加来源，例如 south 或 button3', spellcheck: 'false', 'aria-label': '添加来源' });
  const doAdd = () => { const v = add.value.trim().toLowerCase(); if (!v) return; if (!parts.includes(v)) parts.push(v); setMap(parts.join(', ')); };
  add.addEventListener('keydown', e => { if (e.key === 'Enter') { e.preventDefault(); doAdd(); } });
  add.addEventListener('change', doAdd);
  ed.append(el('div', { class: 'ed-grid' }, listenBox, chips),
    el('div', { class: 'ed-acts' }, add, el('span', { class: 'grow' }),
      el('button', { class: 'linkbtn', text: '恢复默认', onclick: () => setMap('') }),
      el('button', { class: 'linkbtn', text: '不映射', onclick: () => setMap('none') })));
}

/* keyboard binding: the next key pressed */
function listenKey(k) { st.listen = { kind: 'key', key: k }; renderEditor(); }
function keyName(e) {
  const k = e.key, c = e.code;
  if (/^Key[A-Z]$/.test(c)) return c.slice(3);
  if (/^Digit[0-9]$/.test(c)) return c.slice(5);
  if (/^Numpad[0-9]$/.test(c)) return 'NUM' + c.slice(6);
  if (/^F([1-9]|1[0-2])$/.test(k)) return k;
  const m = { ArrowLeft: 'LEFT', ArrowUp: 'UP', ArrowRight: 'RIGHT', ArrowDown: 'DOWN', Enter: 'ENTER', NumpadEnter: 'ENTER', Backspace: 'BACKSPACE',
    Space: 'SPACE', Tab: 'TAB', ShiftLeft: 'SHIFT', ShiftRight: 'SHIFT', ControlLeft: 'CTRL', ControlRight: 'CTRL', AltLeft: 'ALT', AltRight: 'ALT' };
  if (m[c]) return m[c];
  if (e.keyCode > 0 && e.keyCode < 256) return '0x' + e.keyCode.toString(16).toUpperCase().padStart(2, '0');
  return null;
}
document.addEventListener('keydown', e => {
  if (!st.listen || st.page !== 'pads') return;
  if (e.key === 'Escape') { e.preventDefault(); st.listen = null; renderEditor(); return; }
  if (st.listen.kind !== 'key') return;
  e.preventDefault(); e.stopPropagation();
  const n = keyName(e);
  if (n) { st.pads.keyboard.keys[st.listen.key] = n; padSave(); }
  const k = st.listen.key, b = k.replace(/_(UP|LEFT|DOWN|RIGHT)$/, '');
  /* a stick: go on with the next direction */
  const i = DIRS.findIndex(d => k.endsWith('_' + d));
  st.listen = k !== b && i >= 0 && i < 3 && n ? { kind: 'key', key: `${b}_${DIRS[i + 1]}` } : null;
  renderBinds(); renderEditor();
}, true);

/* pad binding: the next source the pad reports (pad-test output), within 8 s */
let listenTimer;
function listenPad(b) {
  const det = detectedOf(st.target);
  if (!det) return;
  st.listen = { kind: 'pad', b, player: det.player, guid: det.guid };
  clearTimeout(listenTimer);
  listenTimer = setTimeout(() => { if (st.listen && st.listen.kind === 'pad') { st.listen = null; renderEditor(); } }, 8000);
  renderEditor();
}
function onPadSource(player, fresh) {
  const l = st.listen;
  if (!l || l.kind !== 'pad' || l.player !== player || !fresh.length) return;
  const c = devCfg(l.guid, true);
  c.map[l.b] = fresh[0];
  st.listen = null; clearTimeout(listenTimer);
  padSave(); renderBinds(); renderEditor();
  pill(`${TITLE[l.b]} = ${srcLabel(fresh[0])}`, 'ok');
}

/* ---- the game's pad tool: runs while this page is open and the game is not */
async function startPadTest() {
  if (st.page !== 'pads' || st.padTool || st.game.state === 'running') { padNote(); return; }
  try { st.live = {}; st.prevSrc = {}; await invoke('pad_tool', { kind: 'test' }); st.padTool = 'test'; st.padToolT0 = Date.now(); }
  catch (e) { fail(e); }
  padNote();
}
async function stopPadTool() { st.padTool = null; try { await invoke('pad_tool_stop'); } catch (e) { } padNote(); }
listen('pad-devices', p => {
  st.detected = p.devices;
  if (!p.partial) { renderCards(); renderTarget(); renderBinds(); renderSlots(); padNote(); }
});
listen('pad-live', v => {
  const prev = st.prevSrc[v.player] || [];
  const fresh = v.sources.filter(s => !prev.includes(s));
  st.prevSrc[v.player] = v.sources;
  st.live[v.player] = v;
  onPadSource(v.player, fresh);
  paintLive();
});
listen('pad-tool-end', p => {
  if (p.kind === 'map') mapDone();
  if (st.padTool === p.kind) {
    st.padTool = null;
    /* the tool stops by itself after a while: start it again while the page is open (not after a quick failure) */
    if (p.kind === 'test' && Date.now() - st.padToolT0 > 10000) startPadTest();
    padNote();
  }
});
function paintLive() {
  const det = !isKb() && detectedOf(st.target);
  const v = det && st.live[det.player];
  $$('#pad [data-b]').forEach(g => g.classList.toggle('lit', !!v && v.buttons.includes(g.dataset.b)));
  const move = (id, [x, y]) => { const cap = $(`#pad [data-b="${id}"] .cap`); if (cap) cap.setAttribute('transform', `translate(${(x / 32767) * 11} ${(-y / 32767) * 11})`); };
  move('LSTICK', v ? v.l : [0, 0]); move('RSTICK', v ? v.r : [0, 0]);
  $$('#pad [data-b="LSTICK"], #pad [data-b="RSTICK"]').forEach(g => g.classList.toggle('moved', !!v && Math.hypot(...(g.dataset.b === 'LSTICK' ? v.l : v.r)) > 6000));
  /* the zone drawing on the general tab follows whichever left stick moved last */
  const any = Object.values(st.live).pop();
  if (any) { const d = $('#zoneViz b'); d.style.transform = `translate(${(any.l[0] / 32767) * 50}%, ${(-any.l[1] / 32767) * 50}%)`; }
}

function ignoreDevice(guid, name) {
  if (!st.pads.general.ignore.includes(guid)) st.pads.general.ignore.push(guid);
  st.target = 'kb'; st.sel = null;
  renderIgnore(); padSave(); renderPadAll();
  toast(`${name || guid} 不再被游戏使用。可以在“通用”页移除。`);
}

/* ---- general tab */
function rangeFill(inp) { inp.style.setProperty('--p', ((inp.value - inp.min) / (inp.max - inp.min) * 100).toFixed(1) + '%'); }
function sliderOut(inp) { rangeFill(inp); const o = inp.parentElement.querySelector('output'); if (o) o.textContent = (+inp.value).toFixed(inp.step === '0.1' ? 1 : 2); }
function zoneViz() {
  const f = $('#padGeneral'), z = $('#zoneViz');
  z.style.setProperty('--dz', (+f.deadzone.value * 100) + '%');
  z.style.setProperty('--oz', (+f.outer_zone.value * 100) + '%');
}
$('#padGeneral').addEventListener('input', e => {
  const t = e.target, g = st.pads && st.pads.general;
  if (!g) return;
  if (t.type === 'range') { sliderOut(t); g[t.name] = t.value; zoneViz(); }
  else if (t.name === 'rumble') g.rumble = t.checked ? 'on' : 'off';
  else if (t.name === 'layout') g.layout = t.value;
  else return;
  padSave();
  if (t.name === 'layout') renderBinds();
});
$('#padGeneral').addEventListener('submit', e => e.preventDefault());
function renderIgnore() {
  const ul = $('#ignoreList'); ul.replaceChildren();
  st.pads.general.ignore.forEach((g, i) => {
    const n = (detectedOf(g) || devCfg(g) || {}).name;
    ul.append(el('li', {}, el('span', { class: 'guid', text: g }), n ? el('span', { class: 'hint', text: n }) : null,
      el('button', { type: 'button', class: 'linkbtn', text: '移除', onclick: () => { st.pads.general.ignore.splice(i, 1); renderIgnore(); padSave(); } })));
  });
  if (!ul.children.length) ul.append(el('li', { class: 'empty', text: '没有。' }));
}
$('#ignoreAddBtn').addEventListener('click', () => {
  const v = $('#ignoreAdd').value.trim().toLowerCase();
  if (!/^[0-9a-f]{32}$/.test(v)) return toast('GUID 是 32 位十六进制数字，在“按键映射”里选中设备后可以看到。', true);
  if (!st.pads.general.ignore.includes(v)) st.pads.general.ignore.push(v);
  $('#ignoreAdd').value = ''; renderIgnore(); padSave();
});

/* ---- player slots */
function renderSlots() {
  if (!st.pads) return;
  const box = $('#slots'); box.replaceChildren();
  const known = new Map();
  for (const d of st.detected) known.set(d.guid, d.name);
  for (const d of st.pads.devices) if (!known.has(d.guid)) known.set(d.guid, d.name || '');
  st.pads.players.forEach(v => { if (/^[0-9a-f]{32}$/i.test(v) && !known.has(v.toLowerCase())) known.set(v.toLowerCase(), ''); });
  st.pads.players.forEach((v, i) => {
    const sel = el('select', { 'aria-label': `${i + 1}P 的设备`, onchange: e => { st.pads.players[i] = e.target.value; padSave(); } },
      el('option', { value: 'auto', text: '自动', selected: v === 'auto' }), el('option', { value: 'none', text: '无人', selected: v === 'none' }),
      [...known].map(([g, n]) => el('option', { value: g, text: `${n || '未知设备'}（${g.slice(0, 8)}）`, selected: v.toLowerCase() === g })));
    const now = st.detected.find(d => d.player === i + 1);
    box.append(el('div', { class: 'slot', 'data-p': i + 1 }, el('span', { class: 'sn', text: `${i + 1}P` }), sel,
      el('span', { class: 'who', text: now ? now.name : st.padTool === 'test' ? '空' : '' })));
  });
}

/* ---- saving: every change, a moment later */
function collectPads() {
  const p = st.pads;
  return { general: p.general, players: p.players, keyboard: p.keyboard,
    devices: p.devices.map(d => ({ guid: d.guid, name: d.name, layout: d.layout || '', deadzone: d.deadzone || '', outer_zone: d.outer_zone || '',
      trigger_threshold: d.trigger_threshold || '', rumble: d.rumble || '', rumble_strength: d.rumble_strength || '', map: d.map, axis: d.axis, invert: d.invert })) };
}
async function savePads() {
  if (!st.pads) return true;
  try {
    const back = await invoke('controllers_set', { value: collectPads() });
    st.pads.path = back.path;
    pill(savedText('已保存，游戏一秒内读取'), 'ok');
    return true;
  } catch (e) { pill(String(e), 'err'); return false; }
}
const padSave = debounce(savePads, 400);

/* pad-map: the game's own tool asks for each button; the dialog shows what it asks */
const MAP_ASK = { 'LEFT stick pushed RIGHT': '把左摇杆推向右', 'LEFT stick pushed DOWN': '把左摇杆推向下', 'RIGHT stick pushed RIGHT': '把右摇杆推向右', 'RIGHT stick pushed DOWN': '把右摇杆推向下' };
async function runPadMap(d) {
  if (!(await padSave.flush())) return;
  if (st.padTool === 'test') await stopPadTool();
  $('#mapIntro').textContent = `${d.player}P：${d.name}。按提示的键，8 秒内不按会跳过这一项，保留原来的映射。`;
  $('#mapAsk').replaceChildren(el('span', { text: '准备中' }));
  $('#mapLog').replaceChildren();
  $('#dlgMap').showModal();
  try { await invoke('pad_tool', { kind: 'map', player: d.player }); st.padTool = 'map'; }
  catch (e) { $('#dlgMap').close(); fail(e); }
}
listen('pad-map', m => {
  if (m.ask && m.waiting) {
    const t = MAP_ASK[m.ask] || '按 ' + (TITLE[m.ask] || m.ask);
    $('#mapAsk').replaceChildren(document.createTextNode(t));
  } else if (m.ask) {
    $('#mapLog').append(el('li', {}, el('b', { text: MAP_ASK[m.ask] || TITLE[m.ask] || m.ask }), m.answer === 'skipped' ? '跳过' : srcLabel(m.answer.replace(/ \(inverted\)$/, ''))));
  } else if (m.text && /no device on player/.test(m.text)) {
    $('#mapAsk').replaceChildren(el('span', { text: '这个玩家槽位上没有手柄' }));
  }
});
function mapDone() {
  const d = $('#dlgMap');
  if (d.open) { $('#mapAsk').replaceChildren(el('span', { text: '已写入' })); setTimeout(() => d.close(), 900); }
  loadPads().then(startPadTest);
}
$('#dlgMap').addEventListener('close', () => { if (st.padTool === 'map') stopPadTool().then(startPadTest); });

/* ================================================================ online */
const NET_STATE = ['未连接', '正在连接', '等待准入', '已加入', '连接失败'];
const NET_ERR = ['', '配置无效：检查地址和房间昵称（4–20 位英文字母、数字、空格或 ._-）。', '游戏还在本地无线会话中，先在游戏里退出本地无线。',
  '网络或系统错误，详见调试页的日志。', '连接超时：检查地址、UDP 端口和网络。', '服务器协议版本不一致。', '房间密码错误。', '房间昵称不合法或已被占用。',
  '服务器房间已满。', '账号或地址被封禁。', '已被房间移出。', '与服务器的连接已断开。', '服务器返回了无效数据。', '发送队列已满。', '虚拟 IP 冲突。'];
const nf = $('#netForm');
let netLoaded = false;
async function loadNetForm() {
  if (netLoaded) return;
  try {
    const v = await invoke('launcher_get'); st.launcher = v;
    nf.host.value = v.net.host; nf.port.value = v.net.port || 24872; nf.nickname.value = v.net.nickname;
    netLoaded = true;
  } catch (e) { fail(e); }
}
function netMsg(t, err) { const n = $('#netMsg'); n.textContent = t || ''; n.classList.toggle('err', !!err); }
function updateNetSteps() {
  const running = st.game.state === 'running', s = running && st.net ? st.net.state : 0;
  const step = !running ? 'game' : s === 3 ? 'enter' : 'join';
  const order = ['game', 'join', 'enter'];
  $$('#netSteps li').forEach(li => {
    const i = order.indexOf(li.dataset.step), at = order.indexOf(step);
    li.className = i < at ? 'done' : i === at ? 'now' : '';
  });
  const busy = s === 1 || s === 2;
  const go = $('#netGo');
  go.textContent = !running ? '前往开始页' : st.game.mode === 'smoke' ? '测试期间不可联机' : !st.bridge ? '等待游戏就绪' : busy ? '正在连接…' : s === 3 ? '已连接服务器' : '连接服务器';
  go.disabled = running && (st.game.mode === 'smoke' || !st.bridge || busy || s === 3);
  $('#netLeave').disabled = !running || !st.bridge || !st.net || s === 0;
  for (const n of ['host', 'port', 'nickname', 'password', 'token']) nf[n].disabled = busy || s === 3;
}
function renderNet(n) {
  st.net = n;
  const s = n ? n.state : 0;
  const rs = $('#roomState'); rs.dataset.s = n ? s : 'off';
  rs.textContent = n ? NET_STATE[s] || s : st.game.state === 'running' ? '未连接' : '游戏未运行';
  $('#roomName').textContent = n && s === 3 ? n.room.name || '房间' : '还没有加入房间';
  $('#roomDesc').textContent = n && s === 3 ? n.room.description : '';
  const meta = $('#roomMeta'); meta.replaceChildren();
  const add = (k, v) => v && meta.append(el('div', {}, el('dt', { text: k }), el('dd', { text: v })));
  if (n && s === 3) {
    add('人数', `${n.members.length} / ${n.room.slots}`); add('虚拟 IP', n.ip);
    add('发送', String(n.sent)); add('接收', String(n.received)); add('丢弃', String(n.dropped));
  }
  if (n && (s === 4 || n.error)) add('原因', NET_ERR[n.error] || n.message);
  const ul = $('#members'); ul.replaceChildren();
  ((n && n.members) || []).forEach((m, i) => {
    ul.append(el('li', { class: m.ip === n.ip ? 'me' : '', 'data-p': (i % 8) + 1 },
      el('span', { class: 'pn', text: `${i + 1}` }),
      el('span', { class: 'mn' }, el('b', { text: m.nickname + (m.display_name ? `（${m.display_name}）` : '') }), el('small', { text: [m.game_name, m.game_version].filter(Boolean).join(' ') || '没有游戏信息' })),
      el('span', { class: 'ip', text: m.ip })));
  });
  $('#membersEmpty').hidden = ul.children.length > 0;
  updateNetSteps();
  applyGame(st.game);
}

async function netSaveFields() {
  try {
    st.launcher = await invoke('launcher_set', { value: { net: { host: nf.host.value.trim(), port: String(nf.port.value), nickname: nf.nickname.value.trim() } } });
  } catch (e) { fail(e); }
}
async function netJoin() {
  const r = await invoke('net_join', { host: nf.host.value.trim(), port: +nf.port.value, nickname: nf.nickname.value.trim(),
    password: nf.password.value, token: nf.token.value });
  if (r.code) netMsg(NET_ERR[r.code] || `加入失败（${r.code}）`, true);
  else netMsg('正在连接服务器');
}
$('#netGo').addEventListener('click', async () => {
  if (st.game.state !== 'running') {
    $('#modes input[value="play"]').checked = true;
    syncMode(); show('start');
    (playerNameError() ? $('#gameName') : $('#btnLaunch')).focus();
    return;
  }
  if ($('#netGo').disabled || !nf.reportValidity()) return;
  netMsg('');
  await netSaveFields();
  try { await netJoin(); } catch (e) { netMsg(String(e), true); }
});
$('#netLeave').addEventListener('click', async () => {
  try { await invoke('net_leave'); netMsg('已离开房间。在游戏里退出本地无线后可以重新加入。'); } catch (e) { netMsg(String(e), true); }
});

/* ================================================================ debug */
$$('#page-debug .subtabs button').forEach(b => b.addEventListener('click', () => {
  $$('#page-debug .subtabs button').forEach(x => x.setAttribute('aria-selected', x === b));
  $('#runOpts').hidden = b.dataset.dsub !== 'run';
  $('#launcherOpts').hidden = b.dataset.dsub !== 'diag';
}));
const LOG_MAX = 4000;
function classOf(line) {
  if (/FATAL|HOST EXCEPTION|GUEST TRAP|UNTRANSLATED|VK_ERROR|failed|ERROR|NOT FOUND|refused/i.test(line)) return 'e';
  if (/WARNING|NO FRAME PRESENTED|late|hold/i.test(line)) return 'w';
  if (/^\[yuzu-net|^console bridge/.test(line)) return 'n';
  return '';
}
function renderLog() {
  const pre = $('#log'), q = $('#logFilter').value.trim().toLowerCase();
  const lines = q ? st.logLines.filter(l => l.toLowerCase().includes(q)) : st.logLines;
  const frag = document.createDocumentFragment();
  for (const l of lines.slice(-LOG_MAX)) { const c = classOf(l); frag.append(c ? el('span', { class: c, text: l + '\n' }) : l + '\n'); }
  pre.replaceChildren(frag);
  if (!lines.length) pre.append(el('span', { class: 'idle', text: st.game.state === 'running' ? '等待输出…' : '游戏运行时，日志显示在这里。' }));
  if ($('#logFollow').checked) pre.scrollTop = pre.scrollHeight;
}
$('#logFilter').addEventListener('input', renderLog);
$('#logClear').addEventListener('click', () => { st.logLines = []; renderLog(); });
$('#netLevel').addEventListener('change', e => invoke('net_level', { level: +e.target.value }).catch(fail));
async function writeDump(full) {
  if (st.game.state !== 'running') return toast('游戏没有在运行。', true);
  if (full && !confirm('完整内存 dump 有几个 GB，写入期间游戏会暂停。继续？')) return;
  toast('正在写 dump');
  try { toast('已写入 ' + await invoke('game_dump', { full })); } catch (e) { fail(e); }
}
$('#dumpMini').addEventListener('click', () => writeDump(false));
$('#dumpFull').addEventListener('click', () => writeDump(true));
$('#openSession').addEventListener('click', () => {
  const d = st.game.session_dir || (st.ov && st.ov.sessions[0] && st.ov.sessions[0].dir);
  if (d) reveal(d); else toast('还没有会话。', true);
});

/* run options of the next start (not saved) */
const ro = $('#runOpts');
function collectRunOpts() {
  const env = [];
  const v = n => (ro[n].value || '').trim();
  if (v('SSBU_SPEED')) env.push(['SSBU_SPEED', v('SSBU_SPEED')]);
  for (const n of ['SSBU_SKIP_MOVIES', 'SSBU_AUDIO_MUTE', 'SSBU_VK_VALIDATE']) if (ro[n].checked) env.push([n, '1']);
  if (v('SSBU_STRICT')) env.push(['SSBU_STRICT', v('SSBU_STRICT')]);
  const dm = [];
  const id = s => s.replace(/[^A-Za-z0-9_]/g, '');
  if (v('dm_p1')) dm.push('p1=' + id(v('dm_p1')));
  if (v('dm_p2')) dm.push('p2=' + id(v('dm_p2')));
  if (v('dm_stage')) dm.push('stage=' + id(v('dm_stage')));
  if (v('dm_form')) dm.push('form=' + v('dm_form'));
  if (v('dm_cpu')) dm.push('cpu=' + Math.min(9, Math.max(1, +v('dm_cpu'))));
  if (v('dm_stock')) dm.push('stock=' + Math.max(1, +v('dm_stock')));
  if (dm.length && v('dm_p2type')) dm.push('p2=' + v('dm_p2type'));
  if (dm.length) env.push(['SSBU_DEBUG_MATCH', dm.join(',')]);
  if (/^\d+s?$/.test(v('SSBU_DEBUG_END'))) env.push(['SSBU_DEBUG_END', v('SSBU_DEBUG_END')]);
  const seconds = v('seconds') ? Math.max(0, +v('seconds')) : null;
  return { env, seconds };
}
function runOptsSummary() {
  const { env, seconds } = collectRunOpts();
  const n = env.length + (seconds != null ? 1 : 0);
  $('#runOptsSummary').textContent = n ? `现在有 ${n} 项。` : '';
  $('#railSub').textContent = n ? `下次启动带 ${n} 项测试选项` : '';
}
ro.addEventListener('input', runOptsSummary);
ro.addEventListener('change', runOptsSummary);
ro.addEventListener('submit', e => e.preventDefault());
$('#runOptsReset').addEventListener('click', () => { ro.reset(); runOptsSummary(); });

/* launcher.ini options: saved on every change */
const lo = $('#launcherOpts');
async function loadLauncherOpts() {
  try {
    const v = await invoke('launcher_get'); st.launcher = v;
    lo.fixed_seed.value = v.fixed_seed; lo.minidump.value = v.minidump; lo.deterministic.checked = v.deterministic === '1';
    lo.fight_hash.checked = v.fight_hash === '1'; lo.zip.value = v.zip; lo.keep_sessions.value = v.keep_sessions;
    lo.exclude_cpus.value = v.exclude_cpus; lo.smoke_seconds.value = v.smoke_seconds;
    const rows = $('#envRows'); rows.replaceChildren();
    for (const e of v.env) addEnvRow(e.key, e.value);
  } catch (e) { fail(e); }
}
function addEnvRow(k = '', val = '') {
  const row = el('div', { class: 'envrow' }, el('input', { value: k, placeholder: 'SSBU_…', 'aria-label': '变量名', spellcheck: 'false' }),
    el('input', { value: val, placeholder: '值', 'aria-label': '值', spellcheck: 'false' }));
  row.append(el('button', { type: 'button', class: 'linkbtn', text: '删除', onclick: () => { row.remove(); loSave(); } }));
  $('#envRows').append(row);
}
$('#envAdd').addEventListener('click', () => { addEnvRow(); $('#envRows .envrow:last-child input').focus(); });
async function saveLauncherOpts() {
  const env = $$('#envRows .envrow').map(r => { const [k, v] = $$('input', r); return { key: k.value.trim(), value: v.value.trim() }; }).filter(x => x.key);
  const value = { fixed_seed: lo.fixed_seed.value.trim() || 'auto', minidump: lo.minidump.value, deterministic: lo.deterministic.checked ? '1' : '0',
    fight_hash: lo.fight_hash.checked ? '1' : '0', zip: lo.zip.value, keep_sessions: lo.keep_sessions.value || '30',
    exclude_cpus: lo.exclude_cpus.value.trim(), smoke_seconds: lo.smoke_seconds.value || '90', env };
  try { st.launcher = await invoke('launcher_set', { value }); pill('已保存', 'ok'); syncMode(); }
  catch (err) { pill(String(err), 'err'); }
}
const loSave = debounce(saveLauncherOpts, 600);
lo.addEventListener('input', loSave);
lo.addEventListener('change', loSave);
lo.addEventListener('submit', e => e.preventDefault());

/* ================================================================ resource packages */
let modState = null, modsBusy = false, modsLoading = false;
const FIGHTERS = { mario:'马力欧',donkey:'森喜刚',link:'林克',samus:'萨姆斯',samusd:'黑暗萨姆斯',yoshi:'耀西',kirby:'卡比',fox:'火狐',pikachu:'皮卡丘',luigi:'路易吉',ness:'奈斯',captain:'飞隼队长',purin:'胖丁',peach:'桃花公主',daisy:'菊花公主',koopa:'酷霸王',popo:'翻越冰山者',nana:'翻越冰山者',sheik:'希克',zelda:'塞尔达',mariod:'马力欧医生',pichu:'皮丘',falco:'佛克',marth:'马尔斯',lucina:'露琪娜',younglink:'幼年林克',ganon:'盖侬多夫',mewtwo:'超梦',roy:'罗伊',chrom:'库洛姆',gamewatch:'Mr. Game & Watch',metaknight:'魅塔骑士',pit:'彼特',pitb:'黑暗彼特',szerosuit:'零装甲萨姆斯',wario:'瓦力欧',snake:'斯内克',ike:'艾克',pzenigame:'杰尼龟',pfushigisou:'妙蛙草',plizardon:'喷火龙',diddy:'迪迪刚',lucas:'卢卡斯',sonic:'索尼克',dedede:'帝帝帝大王',pikmin:'皮克敏＆欧利玛',lucario:'路卡利欧',robot:'机器人',toonlink:'卡通林克',wolf:'沃鲁夫',murabito:'村民',rockman:'洛克人',wiifit:'Wii Fit 教练',rosetta:'罗莎塔＆琪琪',littlemac:'小麦克',gekkouga:'甲贺忍蛙',palutena:'帕露蒂娜',pacman:'吃豆人',reflet:'鲁弗莱',shulk:'修尔克',koopajr:'酷霸王 Jr.',duckhunt:'打猎',ryu:'隆',ken:'肯',cloud:'克劳德',kamui:'神威',bayonetta:'贝优妮塔',inkling:'鱿鱼',ridley:'利德雷',simon:'西蒙',richter:'里希特',krool:'库鲁鲁王',shizue:'西施惠',gaogaen:'炽焰咆哮虎',packun:'吞食花',jack:'Joker',brave:'勇者',buddy:'班卓熊＆卡祖伊鸟',dolly:'特瑞',master:'贝雷特／贝雷丝',tantan:'面面',pickel:'史蒂夫',edge:'萨菲罗斯',eflame:'焰',elight:'光',demon:'三岛一八',trail:'索拉',miifighter:'Mii 格斗类型',miiswordsman:'Mii 剑术类型',miigunner:'Mii 射击类型' };
const fighterName = id => FIGHTERS[id] || id;
const slotNumber = key => Number(key.split(':').pop().slice(1));
async function loadMods() {
  if (modsLoading || modsBusy) return;
  modsLoading = true; syncModsControls();
  $('#modsList').setAttribute('aria-busy', 'true');
  try { modState = await invoke('mods_list'); renderMods(); }
  catch (e) { modState = {supported:false,reason:String(e),packages:[],root:modState?.root}; renderMods(); $('#modsReason').textContent = String(e); $('#modsBadge').textContent = '无法读取'; $('#modsBadge').className = 'badge warning'; fail(e); }
  finally { modsLoading = false; syncModsControls(); $('#modsList').setAttribute('aria-busy', 'false'); }
}
async function manageMods(command, args, message) {
  if (modsBusy || modsLoading) return;
  modsBusy = true; syncModsControls(); pill('正在处理…', 'ok');
  try { modState = await invoke(command, args); renderMods(); pill(message || '已保存，下次启动生效', 'ok'); }
  catch (e) { fail(e); renderMods(); pill('操作未完成', 'err'); }
  finally { modsBusy = false; syncModsControls(); }
}
function syncModsControls() {
  const supported = !!modState?.supported;
  const locked = modsBusy || modsLoading || st.game.state === 'running';
  $$('#modsList [data-mod-write], #modsImportZip, #modsImportFolder').forEach(n => { n.disabled = !supported || locked || n.dataset.unavailable === 'true'; });
  $('#modsOpen').disabled = !supported || modsBusy;
  $('#modsRefresh').disabled = modsBusy || modsLoading;
  $('#modsRunNote').hidden = st.game.state !== 'running';
  $('#modsList').setAttribute('aria-busy', String(modsBusy));
}
function renderMods() {
  if (!modState) return;
  const m = modState, supported = !!m.supported, all = m.packages || [];
  $('#modsBadge').textContent = supported ? '支持资源模组' : '未启用';
  $('#modsBadge').className = 'badge ' + (supported ? 'ok' : 'warning');
  $('#modsReason').textContent = m.load_error ? '加载计划有错误，可关闭或删除相关模组后重试：' + m.load_error : m.disabled_for_run ? '资源模组已被启动设置暂时关闭（SSBU_RESOURCE_MODS=0）' : m.reason;
  $('#modsPath').textContent = m.root || '';
  $('#modsCount').textContent = supported ? `${all.filter(p => p.enabled).length} 个启用 / ${all.length} 个已安装` : '当前包无法加载资源模组。请使用启用模组扩展的构建。';
  const query = $('#modsSearch').value.trim().toLocaleLowerCase();
  const filtered = all.filter(p => [p.id,p.alias,...p.tags,...p.fighters.map(fighterName),...p.fighters,...p.stages].join(' ').toLocaleLowerCase().includes(query));
  const list = $('#modsList');
  const expanded = new Set($$('details[open]', list).map(d => d.dataset.modId));
  list.replaceChildren();
  if (supported && !filtered.length) list.append(el('div', { class:'mods-empty' }, el('h2', { text:all.length ? '没有找到匹配的模组' : '从第一个模组开始' }), el('p', { text:all.length ? '试试角色名、资源类型或模组别名。' : '导入 ZIP 或已解压的文件夹，也可以打开模组目录手动放入。' })));
  for (const p of filtered) {
    const title = p.alias || p.id;
    const toggle = el('input', { type:'checkbox', checked:p.enabled, 'data-mod-write':'', 'aria-label':`启用 ${title}`, onchange:e => manageMods('mods_change', { id:p.id, patch:{enabled:e.target.checked} }) });
    const row = el('article', { class:'mod-row', 'data-enabled':String(p.enabled) });
    const head = el('div', { class:'mod-head' }, el('div', { class:'mod-title' }, el('h2', { text:title }), p.alias ? el('span', { class:'hint', text:p.id }) : null), el('label', { class:'toggle small' }, toggle, el('span', { text:p.enabled ? '启用' : '关闭' })));
    row.append(head);
    const tags = el('div', { class:'mod-tags' });
    for (const f of p.fighters) tags.append(el('span', { class:'mod-tag fighter', text: fighterName(f) }));
    for (const stage of p.stages) tags.append(el('span', { class:'mod-tag fighter', text:stage }));
    for (const tag of p.tags) tags.append(el('span', { class:'mod-tag', text:tag }));
    tags.append(el('span', { class:'hint', text:`${p.resource_count} 项资源` }));
    row.append(tags);
    if (p.costumes.length) row.append(el('p', { class:'mod-slots hint', text:p.costumes.map(k => `${fighterName(k.split(':')[0])} 第 ${slotNumber(k)+1} 套${p.slot_map[k] != null ? ` → 第 ${p.slot_map[k]+1} 套` : ''}`).join(' · ') }));
    if (p.plugins.length) row.append(el('p', { class:'mod-warning', text:'含 Switch / 原生插件依赖，资源加载不代表整包兼容。' }));
    if (p.conflicts.length) row.append(el('p', { class:'mod-warning', text:`与其他启用包有 ${p.conflicts.length} 项同路径覆盖，按后端优先级决定最终内容。` }));
    const details = el('details', { class:'mod-details', 'data-mod-id':p.id, open:expanded.has(p.id) });
    details.append(el('summary', { text:'管理与资源详情' }));
    const alias = el('input', { type:'text', value:p.alias, placeholder:p.id, maxlength:120, 'data-mod-write':'', 'aria-label':`${p.id} 的别名`, onchange:e => manageMods('mods_change', { id:p.id, patch:{alias:e.target.value} }) });
    details.append(el('div', { class:'grid2' }, el('label', { class:'field' }, el('span', { class:'lbl', text:'别名' }), alias), el('div', { class:'field' }, el('span', { class:'lbl', text:'安装位置' }), el('button', { class:'btn', text:'打开此模组文件夹', onclick:() => invoke('mods_open', {id:p.id}).catch(fail) }))));
    if (p.costumes.length) {
      const slots = el('div', { class:'mod-slot-grid' });
      for (const key of p.costumes) {
        const original = slotNumber(key), current = p.slot_map[key] ?? original;
        const input = el('input', { type:'number', min:1, max:256, step:1, value:current+1, 'data-mod-write':'', 'data-unavailable':String(!p.slot_capable), 'aria-label':`${fighterName(key.split(':')[0])} 原第 ${original+1} 套的目标槽位`, onchange:e => {
          const to = Number(e.target.value)-1;
          if (!Number.isInteger(to) || to < 0 || to > 255) { e.target.value = current+1; return fail('请输入 1 到 256 之间的整数'); }
          const map = {...p.slot_map}; if (to === original) delete map[key]; else map[key] = to;
          manageMods('mods_change', { id:p.id, patch:{slot_map:map} });
        }});
        slots.append(el('label', { class:'field' }, el('span', { class:'lbl', text:`${fighterName(key.split(':')[0])} · 原第 ${original+1} 套 → 第几套` }), input));
      }
      details.append(slots, el('p', { class:'hint', text:p.slot_capable ? '保存为虚拟槽位映射，原模组文件保持原样。第 9 套起的额外槽位未完整验收。' : '此包含插件或哈希资源，无法安全地自动更改槽位。' }));
    }
    if (p.paths.length) details.append(el('details', { class:'help' }, el('summary', { text:`资源路径${p.resource_count > p.paths.length ? `（前 ${p.paths.length} 项）` : ''}` }), el('ul', { class:'mod-paths' }, p.paths.map(path => el('li', { text:path })))));
    if (p.conflicts.length) details.append(el('details', { class:'help' }, el('summary', { text:'同路径覆盖' }), el('ul', { class:'mod-paths' }, p.conflicts.map(path => el('li', { text:path })))));
    details.append(el('div', { class:'actions' }, el('button', { class:'btn danger', text:'删除模组', 'data-mod-write':'', onclick:() => {
      if (confirm(`将「${title}」移入模组回收区？可在 mods/.trash 中找回。`)) manageMods('mods_delete', {id:p.id}, '已移入模组回收区');
    }})));
    row.append(details); list.append(row);
  }
  syncModsControls();
}
async function importMods(kind) {
  if (modsBusy || modsLoading || st.game.state === 'running') return;
  modsBusy = true; syncModsControls();
  try {
    const path = await invoke('mods_pick', {kind});
    if (!path) return;
    pill('正在导入…', 'ok');
    modState = await invoke('mods_import', {path}); renderMods();
    pill(`已导入 ${modState.imported?.length || 0} 个模组`, 'ok');
  } catch (e) { fail(e); pill('导入未完成', 'err'); }
  finally { modsBusy = false; syncModsControls(); }
}
$('#modsSearch').addEventListener('input', renderMods);
$('#modsRefresh').addEventListener('click', loadMods);
$('#modsOpen').addEventListener('click', () => invoke('mods_open', {id:null}).catch(fail));
$('#modsImportZip').addEventListener('click', () => importMods('zip'));
$('#modsImportFolder').addEventListener('click', () => importMods('folder'));

/* ================================================================ polling */
async function tick() {
  try {
    const g = await invoke('game_status');
    st.bridge = g.bridge;
    if (g.state === 'running') {
      applyGame(g);
      const r = await invoke('game_log', { after: st.logSeq });
      if (r.lines.length) {
        st.logLines.push(...r.lines);
        if (st.logLines.length > LOG_MAX * 2) st.logLines.splice(0, st.logLines.length - LOG_MAX);
        if (st.page === 'debug') renderLog();
      }
      st.logSeq = r.seq;
      if (g.bridge) {
        try {
          const n = await invoke('net_poll');
          renderNet(n);
        } catch (e) { }
      }
    } else if (st.game.state === 'running') {
      st.net = null;
      applyGame(g);
      renderNet(null);
    }
  } catch (e) { }
  setTimeout(tick, st.game.state === 'running' ? 500 : 1500);
}

/* ================================================================ start */
(async () => {
  await refreshOverview();
  loadDataPaths();
  loadCover();
  runOptsSummary();
  renderLog();
  tick();
})();
