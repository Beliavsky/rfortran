import {examples} from './examples.mjs';
import {locations, serviceURL, summarize, validFilename} from './model.mjs';

const $ = id => document.getElementById(id);
const storageKey = 'rfortran-playground-v1';
const clone = value => JSON.parse(JSON.stringify(value));
let draft = clone(examples[0]), active = 0, busy = false, connected = false, base = '', configured = '';
try { const saved = JSON.parse(localStorage.getItem(storageKey)); if (saved?.files?.length && saved.files.length <= 8 && saved.files.every((f,i) => typeof f.content === 'string' && validFilename(f.name,saved.files,i))) draft = saved; } catch {}
function save() { try { localStorage.setItem(storageKey, JSON.stringify(draft)); } catch {} }
function controls() { $('run').disabled = $('compile').disabled = busy || !connected; $('run').textContent = busy ? 'Working…' : '▶ Compile & run'; }
function position() {
  const before = $('source').value.slice(0, $('source').selectionStart).split('\n');
  $('position').textContent = `Line ${before.length}, column ${before.at(-1).length + 1}`;
}
function lines() { $('lines').textContent = Array.from({length: $('source').value.split('\n').length}, (_, i) => i+1).join('\n'); position(); }
function render() {
  $('files').replaceChildren(...draft.files.map((file,index) => {
    const button = document.createElement('button'); button.className = 'file-tab'; button.textContent = file.name;
    button.role = 'tab'; button.setAttribute('aria-selected', String(index === active));
    button.addEventListener('click', () => {active = index; render();}); return button;
  }));
  $('source').value = draft.files[active].content; $('stdin').value = draft.stdin || '';
  $('remove').disabled = draft.files.length === 1; $('add').disabled = draft.files.length >= 8;
  $('earlier').disabled = active === 0; $('later').disabled = active === draft.files.length - 1;
  lines();
}
$('source').addEventListener('input', () => {draft.files[active].content = $('source').value; save(); lines();});
$('source').addEventListener('click', position); $('source').addEventListener('keyup', position);
$('source').addEventListener('scroll', () => {$('lines').scrollTop = $('source').scrollTop;});
$('source').addEventListener('keydown', event => {
  if (event.key === 'Tab') {event.preventDefault(); const node = $('source'); node.setRangeText('   ', node.selectionStart, node.selectionEnd, 'end'); node.dispatchEvent(new Event('input'));}
});
$('stdin').addEventListener('input', () => {draft.stdin = $('stdin').value; save();});
$('examples').replaceChildren(...examples.map((example,index) => new Option(example.name, index)));
$('examples').addEventListener('change', () => {draft = clone(examples[+$('examples').value]); active = 0; save(); render();});
function nameFile(rename = false) {
  const name = prompt(rename ? 'Rename source file' : 'New source filename (place modules before the main program)', rename ? draft.files[active].name : 'module.f90');
  if (name === null) return;
  if (!validFilename(name, draft.files, rename ? active : -1)) {alert('Use a unique simple filename such as helpers.f90.'); return;}
  if (rename) draft.files[active].name = name;
  else {draft.files.splice(active,0,{name,content:''});}
  save(); render();
}
$('add').onclick = () => nameFile(); $('rename').onclick = () => nameFile(true);
$('remove').onclick = () => {if (draft.files.length > 1) {draft.files.splice(active,1); active = Math.max(0,active-1); save(); render();}};
for (const [id,delta] of [['earlier',-1],['later',1]]) $(id).onclick = () => {const next = active+delta; [draft.files[active],draft.files[next]] = [draft.files[next],draft.files[active]]; active = next; save(); render();};

async function connect() {
  connected = false; controls(); $('connection-dot').className = 'dot';
  $('connection').textContent = 'Connecting to rfort…';
  try {
    base = serviceURL(localStorage.getItem('rfortran-service') || configured);
    const response = await fetch(`${base}/api/health`, {signal: AbortSignal.timeout(10000), cache:'no-store'});
    if (!response.ok) throw new Error('Service unavailable');
    const health = await response.json();
    if (health.compiler !== 'rfort') throw new Error('This is not an rfort service');
    connected = true; $('connection-dot').classList.add('connected');
    $('connection').textContent = `${health.version || 'rfort'}${health.commit ? ` (${health.commit.slice(0,7)})` : ''} · ${health.mode === 'local' ? 'trusted local mode' : 'isolated execution'}`;
    $('status').textContent = 'Ready'; $('status').dataset.state = '';
  } catch {
    $('connection').textContent = 'Execution service not connected'; $('status').textContent = 'Offline';
    $('output').textContent = 'Connect an rfort execution service to compile and run. You can still edit and save your draft here. Use “Service connection” below to set its URL.';
  }
  controls();
}
async function compile(action) {
  if (busy || !connected) return;
  const request = {action,files:clone(draft.files),stdin:draft.stdin || '', optimization:$('optimization').value,warnings:$('warnings').checked};
  busy = true; controls(); $('status').textContent = action === 'run' ? 'Compiling & running…' : 'Compiling…'; $('status').dataset.state = '';
  $('diagnostics').replaceChildren(); $('output').textContent = '';
  try {
    const response = await fetch(`${base}/api/compile`, {method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(request),signal:AbortSignal.timeout(45000)});
    const value = await response.json();
    if (!response.ok) throw new Error(value.error || `Service returned ${response.status}`);
    const summary = summarize(value,action);
    $('output').textContent = summary.text; $('status').textContent = summary.status; $('status').dataset.state = summary.state;
    for (const location of locations(summary.text, request.files)) {
      const button = document.createElement('button'); button.textContent = `${request.files[location.index].name}:${location.line}`;
      button.onclick = () => {
        const index = draft.files.findIndex(f => f.name === request.files[location.index].name);
        if (index < 0) return;
        active = index; render(); const node = $('source');
        const offset = node.value.split('\n').slice(0,location.line-1).reduce((n,line) => n+line.length+1,0) + location.column-1;
        node.focus(); node.setSelectionRange(offset,offset); node.scrollTop = Math.max(0,(location.line-5)*22.75); position();
      }; $('diagnostics').append(button);
    }
  } catch (error) { $('output').textContent = error.name === 'TimeoutError' ? 'The service did not respond in time. The server enforces its own job time limit.' : error.message; $('status').textContent = 'Request failed'; $('status').dataset.state = 'error'; }
  finally {busy = false; controls();}
}
$('compile').onclick = () => compile('compile'); $('run').onclick = () => compile('run');
document.addEventListener('keydown', event => {if ((event.ctrlKey || event.metaKey) && event.key === 'Enter') {event.preventDefault(); compile('run');}});
$('settings').onclick = () => {$('service-url').value = localStorage.getItem('rfortran-service') || configured; $('service-error').textContent = ''; $('service-dialog').showModal();};
$('close-settings').onclick = () => $('service-dialog').close();
$('service-form').onsubmit = event => {event.preventDefault(); try {const url = serviceURL($('service-url').value); localStorage.setItem('rfortran-service',url); $('service-dialog').close(); connect();} catch (error) {$('service-error').textContent = error.message;}};
render();
try {const response = await fetch('./config.json', {cache:'no-store'}); configured = (await response.json()).api_base || '';} catch {}
await connect();
