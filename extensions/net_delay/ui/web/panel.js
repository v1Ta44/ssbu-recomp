'use strict';
const $ = id => document.getElementById(id);
let state = null, requested = null, editing = false, manual = 3, manualChosen = false;
const bridge = window.chrome?.webview;
const send = text => bridge?.postMessage(text);
function commit(frames) {
  if (!Number.isInteger(frames) || frames < -1 || frames > 25) return;
  requested = frames;
  if (frames >= 0) { manual = frames; manualChosen = true; }
  render();
  send(`select:${frames}`);
}
function expanded(value) {
  document.body.dataset.expanded = String(value);
  $('full').hidden = !value;
  if (value) {
    const page = document.querySelector('.page'); page.classList.remove('enter'); void page.offsetWidth; page.classList.add('enter');
    const wipe = document.querySelector('.wipe'); wipe.classList.remove('go'); void wipe.offsetWidth; wipe.classList.add('go');
  }
}
function render() {
  const selected = requested ?? state?.selected ?? -1;
  const auto = selected === -1;
  $('auto').checked = auto; $('manual').checked = !auto;
  $('selected').textContent = auto ? 'AUTO' : String(selected);
  $('unit').textContent = auto ? '自动等待' : '帧';
  const connected = state?.connected && state.backend === 0;
  const controls = ['auto', 'manual', 'minus', 'plus', 'frames', 'reset'];
  controls.forEach(id => { $(id).disabled = !connected; });
  $('minus').disabled ||= !auto && selected === 0;
  $('plus').disabled ||= !auto && selected === 25;
  if (!editing) $('frames').value = String(auto ? manual : selected);
  $('frames').style.setProperty('--p', `${Number($('frames').value) * 4}%`);
  $('original').textContent = state?.calls && state.automatic >= 0 ? `${state.automatic} 帧` : '—';
  $('effective').textContent = state?.calls && state.effective >= 0 ? `${state.effective} 帧` : '—';
  let title = '正在连接控制服务', detail = '准备好后显示当前设置。', badge = 'waiting';
  if (state) {
    if (!state.connected) { title = '控制服务暂未连接'; detail = '正在自动重连，已有设置不会因此改变。'; }
    else if (state.backend !== 0) { title = '当前版本暂不可用'; detail = ['','游戏版本与已验证版本不匹配。','当前程序未提供联机状态。','未能连接游戏计算入口。'][state.backend] || '请查看本次运行记录。'; }
    else if (!state.active) { title = '等待本地无线玩家'; detail = `已选择${auto ? '自动模式' : ` ${selected} 帧`}，玩家接入后等待游戏计算。`; }
    else if (requested !== null || !state.calls || (!auto && (!state.applied || state.effective !== selected || state.applied_revision !== state.selection_revision))) {
      title = '已选择，等待应用'; detail = `本地无线 ${state.peers} 人 · 游戏下次计算时应用。`;
    } else if (auto) { title = '自动模式'; detail = '已停止手动覆盖，等待帧数由游戏决定。'; badge = 'running'; }
    else { title = `已应用 ${state.effective} 帧`; detail = `本地无线 ${state.peers} 人 · 仅调整本机的等待。`; badge = 'running'; }
  }
  $('stateTitle').textContent = title; $('stateDetail').textContent = detail;
  $('connection').dataset.s = badge;
  $('saveStatus').textContent = state?.preferences < 0 ? '设置已选择，但未能保存；本次运行仍然有效。' : state?.preferences > 0 ? '设置已保存，下次启动继续使用。' : '设置只影响本机，修改后自动保存。';
}
bridge?.addEventListener('message', event => {
  const data = event.data;
  if (data.type === 'expanded') { expanded(data.value); return; }
  if (data.type !== 'state') return;
  state = data;
  if (requested !== null && (data.selected === requested || data.command_failed)) requested = null;
  if (requested === null && data.selected >= 0) { manual = data.selected; manualChosen = true; }
  else if (!manualChosen && data.calls && data.automatic >= 0 && data.automatic <= 25) manual = data.automatic;
  render();
});
$('auto').addEventListener('change', () => commit(-1));
$('manual').addEventListener('change', () => commit(manual));
$('reset').addEventListener('click', () => commit(-1));
$('minus').addEventListener('click', () => commit(Math.max(0, (requested ?? state?.selected ?? -1) < 0 ? manual - 1 : (requested ?? state.selected) - 1)));
$('plus').addEventListener('click', () => commit(Math.min(25, (requested ?? state?.selected ?? -1) < 0 ? manual + 1 : (requested ?? state.selected) + 1)));
$('frames').addEventListener('pointerdown', () => { editing = true; });
$('frames').addEventListener('input', () => {
  const value = Number($('frames').value); requested = value; manual = value; $('frames').style.setProperty('--p', `${value * 4}%`); render();
});
$('frames').addEventListener('change', () => { editing = false; commit(Number($('frames').value)); });
$('frames').addEventListener('pointercancel', () => { editing = false; requested = null; render(); });
$('frames').addEventListener('blur', () => { if (editing) { editing = false; commit(Number($('frames').value)); } });
$('close').addEventListener('click', () => send('collapse'));
window.addEventListener('keydown', event => { if (event.key === 'Escape' || event.key === 'F8') { event.preventDefault(); if (!event.repeat) send(event.key === 'F8' ? 'hotkey' : 'collapse'); } });
render(); send('ready');
