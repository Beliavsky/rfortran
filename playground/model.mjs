export function serviceURL(value) {
  if (!value.trim()) return '';
  const url = new URL(value.trim());
  if (url.username || url.password || url.search || url.hash || url.pathname !== '/') throw new Error('Use a service origin without a path or credentials.');
  const loopback = ['localhost', '127.0.0.1', '[::1]'].includes(url.hostname);
  if (url.protocol !== 'https:' && !(url.protocol === 'http:' && loopback)) throw new Error('Use HTTPS, or HTTP on localhost.');
  return url.origin;
}

export function validFilename(name, files, except = -1) {
  return /^[a-z][a-z0-9_-]{0,55}\.(f90|f95|f03|f08)$/i.test(name) && !files.some((f, i) => i !== except && f.name.toLowerCase() === name.toLowerCase());
}

export function locations(text, files) {
  const found = [];
  for (const match of text.matchAll(/([^\s/\\:]+\.(?:f90|f95|f03|f08)):(\d+):(\d+)/gi)) {
    const index = files.findIndex(f => f.name.toLowerCase() === match[1].toLowerCase());
    if (index >= 0 && !found.some(x => x.index === index && x.line === +match[2])) found.push({index, line: +match[2], column: +match[3]});
  }
  return found;
}

export function summarize(result, action) {
  if (result.error) return {state: 'error', status: 'Service error', text: result.error};
  if (!result.build) throw new Error('Invalid response from execution service.');
  const parts = [];
  let failed = false;
  for (const [name, stage] of [['Compiler', result.build], ['Program', result.run]]) {
    if (!stage) continue;
    if (stage.stdout) parts.push(stage.stdout.trimEnd());
    if (stage.stderr) parts.push(stage.stderr.trimEnd());
    if (stage.timed_out) parts.push(`${name} exceeded its time limit.`);
    if (stage.output_limited) parts.push(`${name} exceeded its output limit.`);
    if (stage.exit_code !== 0) parts.push(`${name} exited with code ${stage.exit_code}.`);
    failed ||= stage.exit_code !== 0 || stage.timed_out || stage.output_limited;
  }
  if (!failed && action === 'run' && !result.run) throw new Error('The service did not run the program.');
  const seconds = (result.build.seconds || 0) + (result.run?.seconds || 0);
  return {state: failed ? 'error' : 'success', status: `${failed ? 'Failed' : action === 'compile' ? 'Compiled' : 'Finished'} · ${seconds.toFixed(2)} s`, text: parts.join('\n') || 'Compilation successful.'};
}
