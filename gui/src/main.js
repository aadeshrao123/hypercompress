const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

let state = { compress: null, decompress: null, analyze: null };
let progressStart = 0;

function $(sel) { return document.querySelector(sel); }

function fmt(b) {
  if (b >= 1e9) return (b/1e9).toFixed(2) + ' GB';
  if (b >= 1e6) return (b/1e6).toFixed(1) + ' MB';
  if (b >= 1e3) return (b/1e3).toFixed(1) + ' KB';
  return b + ' B';
}

function basename(p) { return p.replace(/\\/g, '/').split('/').pop() || p; }

function setStatus(msg, type) {
  $('#status').textContent = msg;
  $('#status').className = 'status ' + (type || '');
}

function showProgress(pct, msg) {
  const wrap = $('#progress-wrap');
  wrap.style.display = 'block';
  $('#progress-fill').style.width = pct + '%';
  $('#progress-pct').textContent = Math.round(pct) + '%';
  if (msg) $('#progress-msg').textContent = msg;

  if (progressStart && pct > 5 && pct < 100) {
    const elapsed = (Date.now() - progressStart) / 1000;
    const total = elapsed / (pct / 100);
    const remaining = Math.max(0, total - elapsed);
    if (remaining > 1) {
      $('#progress-msg').textContent = msg + ' — ~' + Math.ceil(remaining) + 's left';
    }
  }
}

function hideProgress() {
  $('#progress-wrap').style.display = 'none';
  $('#progress-fill').style.width = '0%';
  progressStart = 0;
}

listen('progress', (event) => {
  showProgress(event.payload.pct, event.payload.msg);
});

// --- MODAL ---

function showModal(icon, title, rows, isError) {
  $('#modal-icon').textContent = icon;
  $('#modal-title').textContent = title;
  $('#modal').querySelector('.modal').classList.toggle('error', !!isError);

  if (typeof rows === 'string') {
    $('#modal-body').innerHTML = `<p style="text-align:center;color:var(--muted);font-size:14px">${rows}</p>`;
  } else {
    $('#modal-body').innerHTML = rows.map(([label, value, big]) =>
      `<div class="modal-row${big ? ' big' : ''}"><span class="ml">${label}</span><span class="mv">${value}</span></div>`
    ).join('');
  }

  $('#modal').style.display = 'flex';
}

$('#modal-ok').addEventListener('click', () => {
  $('#modal').style.display = 'none';
});

function showError(msg) {
  showModal('✕', 'Error', msg, true);
}

function showSuccess(title, rows) {
  showModal('✓', title, rows, false);
}

// --- TABS ---

function switchTab(name) {
  document.querySelectorAll('.tab').forEach(t => t.classList.remove('active'));
  document.querySelectorAll('.panel').forEach(p => p.classList.remove('active'));
  document.querySelector(`.tab[data-tab="${name}"]`).classList.add('active');
  $(`#${name}`).classList.add('active');
}

document.querySelectorAll('.tab').forEach(t => {
  t.addEventListener('click', () => switchTab(t.dataset.tab));
});

// --- HELPERS ---

function selectFile(tab, path) {
  state[tab] = path;
  $(`#${tab}-file`).textContent = basename(path);
  $(`#${tab}-file`).style.display = 'block';
  $(`#${tab}-btn`).disabled = false;
}

function resetTab(tab) {
  state[tab] = null;
  $(`#${tab}-file`).style.display = 'none';
  $(`#${tab}-btn`).disabled = true;
  if (tab === 'compress') {
    $('#encrypt-pw').value = '';
    $('#level').value = '6';
  }
  if (tab === 'decompress') {
    $('#decrypt-pw').value = '';
  }
  setStatus('', '');
}


// --- PICKERS ---

$('#compress-drop').addEventListener('click', async () => {
  const folder = await invoke('pick_folder');
  if (folder) { selectFile('compress', folder); return; }
  const file = await invoke('pick_file');
  if (file) selectFile('compress', file);
});

$('#decompress-drop').addEventListener('click', async () => {
  const file = await invoke('pick_hc_file');
  if (file) selectFile('decompress', file);
});

$('#analyze-drop').addEventListener('click', async () => {
  const file = await invoke('pick_file');
  if (file) selectFile('analyze', file);
});

// --- COMPRESS ---

$('#compress-btn').addEventListener('click', async () => {
  if (!state.compress) return;

  const defaultName = basename(state.compress) + '.hc';
  const output = await invoke('pick_save_file', { defaultName });
  if (!output) return;

  const level = parseInt($('#level').value);
  const pw = $('#encrypt-pw').value || '';

  progressStart = Date.now();
  showProgress(0, 'Starting...');
  $('#compress-btn').disabled = true;
  $('#compress-btn').textContent = 'Compressing...';

  try {
    const r = await invoke('compress_file', { path: state.compress, output, level, password: pw });

    const rows = [
      ['Input', basename(r.input_path)],
      ['Output', basename(r.output_path)],
      ['Original', fmt(r.original_size)],
      ['Compressed', fmt(r.compressed_size)],
      ['Ratio', r.ratio.toFixed(1) + ':1', true],
    ];
    if (r.encrypted) rows.push(['Encryption', 'AES-256-GCM']);

    hideProgress();
    showSuccess('Compression Complete', rows);
    resetTab('compress');
  } catch (e) {
    hideProgress();
    showError(String(e));
  }

  $('#compress-btn').disabled = false;
  $('#compress-btn').textContent = 'Compress';
  setStatus('', '');
});

// --- DECOMPRESS ---

$('#decompress-btn').addEventListener('click', async () => {
  if (!state.decompress) return;

  const output = await invoke('pick_extract_folder');
  if (!output) {
    // use same dir as .hc file
    const dir = state.decompress.replace(/\\/g, '/').split('/').slice(0, -1).join('/');
    const name = basename(state.decompress).replace(/\.hc$/, '');
    var extractTo = dir + '/' + name;
  } else {
    var extractTo = output;
  }

  const pw = $('#decrypt-pw').value || '';

  progressStart = Date.now();
  showProgress(0, 'Starting...');
  $('#decompress-btn').disabled = true;
  $('#decompress-btn').textContent = 'Extracting...';

  try {
    const r = await invoke('decompress_file', { path: state.decompress, output: extractTo, password: pw });

    hideProgress();
    showSuccess('Extraction Complete', [
      ['Output', r.output_path],
      ['Total Size', fmt(r.size)],
      ['Files', String(r.file_count)],
    ]);
    resetTab('decompress');
  } catch (e) {
    hideProgress();
    showError(String(e));
  }

  $('#decompress-btn').disabled = false;
  $('#decompress-btn').textContent = 'Extract';
  setStatus('', '');
});

// --- ANALYZE ---

$('#analyze-btn').addEventListener('click', async () => {
  if (!state.analyze) return;
  setStatus('Analyzing...', 'working');

  try {
    const r = await invoke('analyze_file', { path: state.analyze });
    showSuccess('File Analysis', [
      ['File', basename(state.analyze)],
      ['Size', fmt(r.size)],
      ['Detected Type', r.detected_type],
      ['Entropy', r.entropy.toFixed(3) + ' bits/byte'],
      ['ASCII', (r.ascii_ratio * 100).toFixed(1) + '%'],
      ['Zero Bytes', (r.zero_ratio * 100).toFixed(1) + '%'],
      ['Unique Bytes', String(r.unique_bytes)],
    ]);
  } catch (e) {
    showError(String(e));
  }
  setStatus('', '');
});

// --- STARTUP ARGS ---

(async () => {
  try {
    const args = await invoke('get_startup_args');
    if (!args.path) return;
    if (args.action === 'compress') { switchTab('compress'); selectFile('compress', args.path); }
    else if (args.action === 'decompress') { switchTab('decompress'); selectFile('decompress', args.path); }
  } catch (e) {}
})();
