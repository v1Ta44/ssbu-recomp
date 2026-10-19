'use strict';
/* Only local status is polled. Remote checks, downloads and installation require user action. */
let updateView = null;
let updateConfigLoaded = false;
const updateTime = n => n ? new Date(n * 1000).toLocaleString('zh-CN') : '';
const updateSize = n => n >= 1024 ** 3 ? `${(n / 1024 ** 3).toFixed(1)} GB` : `${(n / 1024 ** 2).toFixed(1)} MB`;
function paintUpdateSource() {
  const sources = updateView?.latest?.artifact?.sources || [];
  const s = sources[Number($('#updateSource').value)];
  $('#updateDownload').textContent = s?.kind === 'browser' ? '打开备用下载' : '下载并准备更新';
  $('#updateSourceNote').textContent = s ? [s.note, s.code ? `提取码：${s.code}` : '', s.kind === 'browser' ? '在浏览器下载后，回到这里选择“从文件更新”。' : '来源失败时会尝试其他直链来源。'].filter(Boolean).join(' ') : '';
}
function paintUpdates(v) {
  updateView = v;
  const active = st.game.state === 'running' || !!st.padTool;
  $('#updateCurrent').textContent = `当前：${v.current.version} · ${v.current.build_id}`;
  const technicalError = v.phase === 'error' && (v.message.includes('https://') || v.message.length > 180);
  $('#updateMessage').textContent = technicalError ? `${v.message.split('。')[0]}。可以稍后重试或从文件更新。` : v.message;
  $('#updateErrorDetails').hidden = !technicalError;
  $('#updateErrorText').textContent = technicalError ? v.message : '';
  $('#updateCard').dataset.error = String(v.phase === 'error');
  $('#updateChecked').textContent = v.check.last_success ? `上次成功检查：${updateTime(v.check.last_success)}${v.check.last_source ? ' · ' + new URL(v.check.last_source).hostname : ''}` : '尚无成功的在线检查记录。';
  const progress = $('#updateProgress'); progress.hidden = !v.busy || !v.total; progress.max = Math.max(v.total, 1); progress.value = v.done;
  $('#updateCheck').disabled = v.busy || !v.supported;
  $('#updateImport').disabled = v.busy || !v.supported;
  $('#updateDownload').disabled = v.busy || !v.latest?.newer || !v.latest?.compatible;
  $('#updateCancel').hidden = v.phase !== 'downloading';
  $('#updateReady').hidden = !v.ready;
  $('#updateInstall').disabled = v.busy || active;
  if (v.ready) {
    $('#updateReadyTitle').textContent = `准备安装 ${v.ready.version.version}`;
    $('#updateReadyNote').textContent = `${v.ready.files} 个文件，${updateSize(v.ready.bytes)}。${active ? '请先关闭游戏和手柄工具。' : '安装期间控制台会暂时关闭。'}`;
  }
  $('#updateNotes').textContent = v.latest ? `${v.latest.version.version}\n${v.latest.notes}${!v.latest.compatible ? '\n此版本与当前功能组合不同，暂不能安装。' : ''}${v.latest.sources_delayed ? '\n来源可能尚未同步到已知的新版本。' : ''}` : '检查更新后查看版本说明。';
  const sources = v.latest?.artifact?.sources || [];
  $('#updateSourcesBox').hidden = !sources.length;
  const select = $('#updateSource'); const previous = select.value;
  const options = sources.map((s, index) => ({ index, label: `${s.name}${s.kind === 'browser' ? '（浏览器下载）' : ''}` }));
  const fingerprint = JSON.stringify(options);
  if (select.dataset.options !== fingerprint) {
    select.replaceChildren(...options.map(s => el('option', {value:String(s.index),text:s.label})));
    select.dataset.options = fingerprint;
    if (options.some(o => String(o.index) === previous)) select.value = previous;
  }
  paintUpdateSource();
  $('#updateRollback').hidden = !v.rollback;
  $('#updateRollback').disabled = v.busy || active;
  const resultLabels = {install:'更新已成功安装',rollback:'已恢复上一版本',recover:'未完成的更新已恢复'};
  const resultMatch = /^(install|rollback|recover): success at (\d+)$/.exec(v.last_result);
  $('#updateLastResult').textContent = resultMatch ? `${resultLabels[resultMatch[1]]} · ${updateTime(Number(resultMatch[2]))}` : v.last_result ? `上次安装结果：${v.last_result}` : '';
  if (!updateConfigLoaded) {
    $('#updateStartup').checked = v.config.startup_check;
    $('#updateFeeds').value = v.config.manifest_urls.join('\n');
    updateConfigLoaded = true;
  }
}
async function refreshUpdates() {
  try { paintUpdates(await invoke('update_status')); }
  catch (e) { $('#updateMessage').textContent = String(e); $('#updateCard').dataset.error = 'true'; }
}
async function updateAction(command, args) {
  try { await invoke(command, args); await refreshUpdates(); }
  catch (e) { $('#updateMessage').textContent = String(e); $('#updateCard').dataset.error = 'true'; }
}
$('#updateSource').addEventListener('change', paintUpdateSource);
$('#updateCheck').addEventListener('click', () => updateAction('update_check', {automatic:false}));
$('#updateImport').addEventListener('click', async () => {
  try { const path = await invoke('update_pick'); if (path) await updateAction('update_import', {path}); } catch (e) { fail(e); }
});
$('#updateDownload').addEventListener('click', () => {
  const index = Number($('#updateSource').value);
  const source = updateView?.latest?.artifact?.sources[index];
  if (!source) return;
  updateAction(source.kind === 'browser' ? 'update_browser' : 'update_download', source.kind === 'browser' ? {index} : {source:source.url});
});
$('#updateCancel').addEventListener('click', () => updateAction('update_cancel'));
$('#updateInstall').addEventListener('click', () => {
  if (confirm('安装会关闭控制台，完成后自动重新打开。现在安装吗？')) updateAction('update_apply', {rollback:false});
});
$('#updateRollback').addEventListener('click', () => {
  if (confirm('退出控制台并恢复上一次更新前的程序？存档和个人设置会保留。')) updateAction('update_apply', {rollback:true});
});
async function saveUpdateConfig() {
  const config = { startup_check:$('#updateStartup').checked, manifest_urls:$('#updateFeeds').value.split(/\r?\n/).map(s => s.trim()).filter(Boolean) };
  try { await invoke('update_config', {config}); toast('更新来源设置已保存。'); } catch (e) { fail(e); }
}
$('#updateStartup').addEventListener('change', saveUpdateConfig);
$('#updateSaveFeeds').addEventListener('click', saveUpdateConfig);
(async function updatePoll() {
  await refreshUpdates();
  setTimeout(updatePoll, updateView?.busy ? 800 : 5000);
})();
(async () => {
  try { const v = await invoke('update_status'); if (v.config.startup_check) await updateAction('update_check', {automatic:true}); } catch (_) { }
})();
