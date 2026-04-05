const { invoke } = window.__TAURI__.core;
const { open } = window.__TAURI__.dialog;

let state = { compress: null, decompress: null, analyze: null };

function $(sel) { return document.querySelector(sel); }
function fmt(bytes) {
  if (bytes >= 1e6) return (bytes/1e6).toFixed(1) + ' MB';
  if (bytes >= 1e3) return (bytes/1e3).toFixed(1) + ' KB';
  return bytes + ' B';
}

function setStatus(msg, type = '') {
  const el = $('#status');
  el.textContent = msg;
  el.className = 'status ' + type;
}

function showResult(id, rows) {
  const el = $(id);
  el.innerHTML = rows.map(([label, value, highlight]) =>
    `<div class="result-row${highlight ? ' highlight' : ''}">
      <span class="label">${label}</span>
      <span class="value">${value}</span>
    </div>`
  ).join('');
}

// Tab switching
document.querySelectorAll('.tab').forEach(tab => {
  tab.addEventListener('click', () => {
    document.querySelectorAll('.tab').forEach(t => t.classList.remove('active'));
    document.querySelectorAll('.panel').forEach(p => p.classList.remove('active'));
    tab.classList.add('active');
    $(`#${tab.dataset.tab}`).classList.add('active');
  });
});

// Level slider
$('#level').addEventListener('input', e => {
  $('#level-val').textContent = e.target.value;
});

// File picker for each drop zone
async function pickFile(type) {
  const filters = type === 'decompress'
    ? [{ name: 'HyperCompress', extensions: ['hc'] }]
    : [{ name: 'All Files', extensions: ['*'] }];

  const path = await open({ multiple: false, filters, directory: type === 'compress' ? undefined : false });
  if (!path) return;

  state[type] = path;
  $(`#${type}-file`).textContent = path;
  $(`#${type}-file`).style.display = 'block';
  $(`#${type}-btn`).disabled = false;
}

// Also allow picking folders for compress
$('#compress-drop').addEventListener('click', async () => {
  const choice = confirm('Select a folder? (Cancel for file)');
  if (choice) {
    const path = await open({ directory: true });
    if (path) { state.compress = path; $('#compress-file').textContent = path; $('#compress-file').style.display='block'; $('#compress-btn').disabled=false; }
  } else {
    await pickFile('compress');
  }
});

$('#decompress-drop').addEventListener('click', () => pickFile('decompress'));
$('#analyze-drop').addEventListener('click', () => pickFile('analyze'));

// Compress
$('#compress-btn').addEventListener('click', async () => {
  if (!state.compress) return;
  const level = parseInt($('#level').value);
  const pw = $('#encrypt-pw').value || null;

  setStatus('Compressing...', 'working');
  $('#compress-btn').disabled = true;

  try {
    const r = await invoke('compress_file', {
      path: state.compress, output: null, level, password: pw
    });
    showResult('#compress-result', [
      ['Input', r.input_path],
      ['Output', r.output_path],
      ['Original', fmt(r.original_size)],
      ['Compressed', fmt(r.compressed_size)],
      ['Ratio', r.ratio.toFixed(1) + 'x', true],
      ...(r.encrypted ? [['Encrypted', 'AES-256-GCM']] : []),
    ]);
    setStatus('Done!', 'success');
  } catch (e) {
    setStatus(e, 'error');
  }
  $('#compress-btn').disabled = false;
});

// Decompress
$('#decompress-btn').addEventListener('click', async () => {
  if (!state.decompress) return;
  const pw = $('#decrypt-pw').value || null;

  setStatus('Extracting...', 'working');
  $('#decompress-btn').disabled = true;

  try {
    const r = await invoke('decompress_file', {
      path: state.decompress, output: null, password: pw
    });
    showResult('#decompress-result', [
      ['Output', r.output_path],
      ['Size', fmt(r.size)],
    ]);
    setStatus('Extracted!', 'success');
  } catch (e) {
    setStatus(e, 'error');
  }
  $('#decompress-btn').disabled = false;
});

// Analyze
$('#analyze-btn').addEventListener('click', async () => {
  if (!state.analyze) return;
  setStatus('Analyzing...', 'working');

  try {
    const r = await invoke('analyze_file', { path: state.analyze });
    showResult('#analyze-result', [
      ['Detected Type', r.detected_type],
      ['Entropy', r.entropy.toFixed(3) + ' bits/byte'],
      ['ASCII', (r.ascii_ratio * 100).toFixed(1) + '%'],
      ['Unique Bytes', r.unique_bytes],
    ]);
    setStatus('', '');
  } catch (e) {
    setStatus(e, 'error');
  }
});
