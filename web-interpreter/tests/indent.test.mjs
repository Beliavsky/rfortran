import test from "node:test";
import assert from "node:assert/strict";
import vm from "node:vm";
import {readFileSync} from "node:fs";
const context=vm.createContext({});
vm.runInContext(readFileSync(new URL("../indent.js",import.meta.url),"utf8"),context);
const indent=context.RfortIndent;
function levels(source) {
  const state=indent.start();
  return source.split("\n").map(line=>{
    const spaces=indent.indent(state,line);
    indent.advance(state,line);
    return spaces;
  });
}
test("three-space nesting, named loops, ELSE and compact ENDIF",()=>{
  assert.deepEqual(levels('program p\ndo_i: do i=1,3\nif(i>1) then\nprint *,i\nelse if(i==1) then\nprint *,0\nelse\nprint *,-1\nendif\nend do do_i\nend program p'),
    [0,3,6,9,6,9,6,9,6,3,0]);
});
test("SELECT branches, derived types and contained procedures",()=>{
  assert.deepEqual(levels('module m\ntype :: t\ninteger::i\nend type\ncontains\npure real function f(x)\nselect case(x)\ncase(1)\nf=1\ncase default\nf=0\nend select\nend function\nend module'),
    [0,3,6,3,0,3,6,9,12,9,12,6,3,0]);
});
test("strings, comments, single-line IF/WHERE and keyword variables do not open blocks",()=>{
  assert.deepEqual(levels('program p\nprint *, "do; if(x) then ! end"\nprint *, \'it\'\'s end do\'\n! do while(.true.)\nif(x) print *,1\nwhere(a>0) a=1\ntype(t)::x\nend=1\ndo=2\nprint *,0\nend program'),
    [0,3,3,3,3,3,3,3,3,3,0]);
});
test("continuations, semicolons and blank lines preserve block state",()=>{
  assert.deepEqual(levels('program p\nif (a .and. &\n! comment\n& b) then\nprint *, &\n& a\nendif; do i=1,2\nprint *,i\nenddo\nend'),[0,3,6,6,6,9,3,6,3,0]);
  assert.equal(indent.after('program p\nif (x) then'),"      ");
  assert.equal(indent.after('program p\nif (x) then','end if'),"   ");
});
test("WHERE, FORALL, INTERFACE, BLOCK and ASSOCIATE pairs",()=>{
  assert.deepEqual(levels('interface f\nmodule procedure g\nend interface\nblock\nassociate(x=>y)\nwhere(x>0)\nx=1\nelsewhere\nx=0\nend where\nend associate\nend block'),[0,3,0,0,3,6,9,6,9,6,3,0]);
});
test("CodeMirror mode delegates highlighting and copies indentation independently",()=>{
  const modes=new Map(),mimes=new Map();
  function CodeMirror() {}
  CodeMirror.defineMode=(name,factory)=>modes.set(name,factory);
  CodeMirror.defineMIME=(mime,name)=>mimes.set(mime,name);
  CodeMirror.getMode=(config,name)=>modes.get(mimes.get(name)||name)(config);
  CodeMirror.startState=mode=>mode.startState();
  CodeMirror.copyState=(mode,state)=>({...state});
  const c=vm.createContext({CodeMirror});
  vm.runInContext(readFileSync(new URL('../vendor/fortran.min.js',import.meta.url),'utf8'),c);
  vm.runInContext(readFileSync(new URL('../indent.js',import.meta.url),'utf8'),c);
  const mode=CodeMirror.getMode({indentUnit:3},'rfort-fortran');
  const state=mode.startState();
  function line(string,state) {
    const stream={string,pos:0,start:0,sol(){return this.pos===0;},
      next(){return this.string[this.pos++];},
      skipToEnd(){this.pos=this.string.length;},
      eatWhile(pattern){while(this.pos<this.string.length && pattern.test(this.string[this.pos]))this.pos++;},
      eatSpace(){const start=this.pos;this.eatWhile(/\s/);return this.pos>start;},
      current(){return this.string.slice(this.start,this.pos);},
      match(pattern){const m=this.string.slice(this.pos).match(pattern);if(!m || m.index!==0)return false;this.pos+=m[0].length;return m;}};
    const tokens=[];
    while(stream.pos<string.length) {stream.start=stream.pos;tokens.push(mode.token(stream,state));}
    return tokens;
  }
  assert.ok(line('program p',state).includes('keyword'));
  line('do i=1,3',state);
  assert.equal(mode.indent(state,'print *,i'),6);
  assert.equal(mode.indent(state,'enddo'),3);
  const other=mode.copyState(state);line('enddo',other);
  assert.equal(mode.indent(other,'print *,i'),3);
  assert.equal(mode.indent(state,'print *,i'),6);
  assert.ok(line('print *, "end do" ! comment',state).includes('string'));
});
