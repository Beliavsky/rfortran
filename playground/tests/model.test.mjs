import test from 'node:test';
import assert from 'node:assert/strict';
import {locations, serviceURL, summarize, validFilename} from '../model.mjs';

test('only HTTPS or loopback HTTP services', () => {
  assert.equal(serviceURL('https://rfort.example.org/'), 'https://rfort.example.org');
  assert.equal(serviceURL('http://localhost:8080'), 'http://localhost:8080');
  for (const url of ['http://remote.example', 'javascript:alert(1)', 'https://user:pass@example.org', 'https://example.org/path']) assert.throws(() => serviceURL(url));
});
test('filenames are unique and cannot be options or paths', () => {
  assert.equal(validFilename('main.f90', []), true);
  for (const name of ['../main.f90', '-O2.f90', 'sub/main.f90', 'main.exe']) assert.equal(validFilename(name, []), false);
  assert.equal(validFilename('MAIN.F90', [{name:'main.f90'}]), false);
});
test('diagnostics map Windows and Linux paths to editor files', () => {
  assert.deepEqual(locations('C:\\tmp\\main.f90:3:7\n/tmp/main.f90:3:7', [{name:'main.f90'}]), [{index:0,line:3,column:7}]);
});
test('failed compilation and run limits are visible', () => {
  const build = {exit_code:0,stdout:'',stderr:'',seconds:0.1};
  assert.equal(summarize({build,run:{exit_code:0,stdout:'42\n',seconds:0.2}},'run').text,'42');
  assert.equal(summarize({build:{...build,exit_code:1,stderr:'Bad source'},run:null},'run').state,'error');
  assert.match(summarize({build,run:{exit_code:-9,timed_out:true}},'run').text,/time limit/);
  assert.throws(() => summarize({build,run:null},'run'));
});
