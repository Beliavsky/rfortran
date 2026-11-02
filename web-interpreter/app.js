// Adapted from Beliavsky/ofort (MIT); see THIRD-PARTY-NOTICES.txt.
"use strict";
const byId = id => document.getElementById(id);
const examples = {
  hello: 'program hello\n   implicit none\n   print *, "Hello from rfortran in your browser!"\nend program hello\n',
  arrays: 'program arrays\n   implicit none\n   integer :: i\n   integer, parameter :: dp = kind(0.0d0)\n   real(kind=dp) :: x(5)\n   x = [(real(i, kind=dp), i=1,5)]\n   print *, "x =", x\n   print *, "sum =", sum(x)\n   print *, "mean =", sum(x)/size(x)\nend program arrays\n',
  input: 'program input_demo\n   implicit none\n   integer :: n, ios\n   read(*, *, iostat=ios) n\n   if (ios == 0) then\n      print *, "Your number squared:", n*n\n   else\n      print *, "Supply an integer in the standard input box."\n   end if\nend program input_demo\n'
};
let capabilities = {autoDeclare:false, autoEnd:false};
let worker = null;
let timer = null;
let lastRun = null;
let busy = false;
let programSource = examples.hello;
let commandHistory = [];
let historyIndex = 0;
let sessionBaseline = "";
let sourceDirty = false;
let settingSource = false;
let replayDraft = null;
let replayNeedsCorrection = false;
let blockEnds = [];
const isRepl = () => byId("mode").value === "repl";
function updateSourceControls() {
  byId("clear-code").disabled = busy;
  sourceDirty = isRepl() && (replayNeedsCorrection || sourceText() !== sessionBaseline);
  byId("source-edits").hidden = !sourceDirty;
  byId("discard-edits").disabled = busy;
  byId("repl-submit").disabled = busy || sourceDirty;
  byId("repl-command").disabled = busy || sourceDirty;
  byId("auto-declare").disabled = busy || !capabilities.autoDeclare;
  byId("auto-end").disabled = busy || !capabilities.autoEnd;
  byId("finish-block").disabled = busy || sourceDirty || !blockEnds.length;
  byId("finish-block").hidden = !isRepl() || !byId("auto-end").checked || !blockEnds.length;
  byId("end-preview").hidden = !isRepl() || !byId("auto-end").checked || !blockEnds.length;
  byId("suggested-ends").textContent = blockEnds.join("\n");
  if (editor) editor.setOption("readOnly", busy);
  else byId("source").readOnly = busy;
}
function finish(status, keepWorker = false) {
  if (!keepWorker) {
    blockEnds = [];
    if (worker) worker.terminate();
    worker = null;
  }
  clearTimeout(timer);
  busy = false;
  byId("run").disabled = false;
  byId("stop").disabled = true;
  byId("repl-submit").disabled = false;
  byId("mode").disabled = false;
  byId("clear-session").disabled = false;
  byId("status").textContent = status;
  updateSourceControls();
}
function readDraft(key, fallback) {
  try { return localStorage.getItem(key) ?? fallback; } catch { return fallback; }
}
function saveDraft(key, text) {
  try { localStorage.setItem(key, text); } catch { /* Private/full storage is optional. */ }
}
byId("source").value = readDraft("rfortran.web.source", examples.hello);
byId("stdin").value = readDraft("rfortran.web.stdin", "");
byId("stdin").addEventListener("input", () => saveDraft("rfortran.web.stdin", byId("stdin").value));
const editor = typeof CodeMirror === "function" ? CodeMirror.fromTextArea(byId("source"), {
  mode: "rfort-fortran",
  theme: "ofort",
  lineNumbers: true,
  indentUnit: 3,
  tabSize: 3,
  indentWithTabs: false,
  smartIndent: true,
  electricChars: true,
  lineWrapping: true,
  extraKeys: {
    "Ctrl-Enter": () => { if (!busy) byId("run").click(); },
    "Cmd-Enter": () => { if (!busy) byId("run").click(); },
    "Tab": cm => {
      if (cm.somethingSelected()) cm.indentSelection("add");
      else cm.replaceSelection("   ", "end");
    },
    "Shift-Tab": cm => cm.indentSelection("subtract")
  }
}) : null;
const sourceText = () => editor ? editor.getValue() : byId("source").value;
function setSource(text) {
  settingSource = true;
  replayNeedsCorrection = false;
  if (editor) editor.setValue(text);
  else byId("source").value = text;
  if (isRepl()) sessionBaseline = text;
  settingSource = false;
  updateSourceControls();
}
function sourceChanged() {
  if (!settingSource) {
    updateSourceControls();
    if (!isRepl()) saveDraft("rfortran.web.source", sourceText());
  }
}
if (editor) {
  editor.on("change", sourceChanged);
  editor.on("inputRead", (cm,change) => {
    if (change.origin !== "paste") return;
    cm.operation(() => {
      for (let line=change.from.line;line<change.from.line+change.text.length;line++) cm.indentLine(line,"smart");
    });
  });
} else byId("source").addEventListener("input", sourceChanged);
byId("discard-edits").addEventListener("click", () => {
  if (!busy) setSource(sessionBaseline);
});
byId("auto-end").addEventListener("change", updateSourceControls);
byId("finish-block").addEventListener("click", () => {
  if (busy || sourceDirty || !blockEnds.length) return;
  send({type:"repl-submit", command:blockEnds[0]});
});
function appendText(id, text) {
  if (!text) return;
  const pane = byId(id);
  const combined = pane.textContent + (pane.textContent ? "\n" : "") + text;
  pane.textContent = combined.length > 1000000 ? "[Earlier output omitted]\n" + combined.slice(-1000000) : combined;
  pane.scrollTop = pane.scrollHeight;
}
byId("clear-code").addEventListener("click", () => {
  if (editor) {
    const last = editor.lastLine();
    editor.replaceRange("", {line:0, ch:0}, {line:last, ch:editor.getLine(last).length}, "+clear");
    editor.focus();
  } else {
    const source = byId("source");
    source.focus();
    source.select();
    // Use the browser's editing history when the rich editor is unavailable.
    document.execCommand("delete");
  }
  sourceChanged();
});
byId("focus").addEventListener("click", () => {
  const focused = document.body.classList.toggle("focus-mode");
  byId("focus").textContent = focused ? "Exit focus mode" : "Focus mode";
  byId("focus").setAttribute("aria-pressed", String(focused));
  if (editor) editor.refresh();
});
byId("example").addEventListener("change", event => {
  if (editor) editor.setValue(examples[event.target.value]);
  else byId("source").value = examples[event.target.value];
  if (event.target.value === "input") byId("stdin").value = "7\n";
  sourceChanged();
  saveDraft("rfortran.web.stdin", byId("stdin").value);
});
byId("run").addEventListener("click", () => {
  if (busy) return;
  if (isRepl()) {
    if (!confirm("Restart the interpreter and run the source from the beginning? Current values will be discarded. Random-number calls and file writes will run again.")) return;
    byId("output").textContent = "";
    byId("errors").textContent = "";
    send({type:"repl-replay", source:sourceText()});
  }
  else {
    byId("output").textContent = "";
    byId("errors").textContent = "";
    send({source:sourceText()});
  }
});
byId("stop").addEventListener("click", () => {
  if (lastRun) lastRun.status = "Stopped";
  finish("Stopped");
  if (isRepl()) {
    appendText("errors", "Stopped. The REPL interpreter and temporary files were discarded.");
    setSource("");
    byId("variables").textContent = "(no variables)";
  }
});
byId("source").addEventListener("keydown", event => {
  if ((event.ctrlKey || event.metaKey) && event.key === "Enter") {
    event.preventDefault();
    if (!busy) byId("run").click();
  }
  if (event.key === "Tab") {
    event.preventDefault();
    const editor = event.target;
    editor.setRangeText("   ", editor.selectionStart, editor.selectionEnd, "end");
    sourceChanged();
  } else if (event.key === "Enter" && !event.ctrlKey && !event.metaKey && !event.isComposing) {
    event.preventDefault();
    indentNewline(event.target);
    sourceChanged();
  }
});
byId("download").addEventListener("click", () => {
  let source = sourceText();
  if (isRepl()) {
    // The interpreter disables implicit typing without visible boilerplate.
    // Downloads retain an explicit declaration rule after USE/IMPORT lines.
    const lines = source.split(/\r?\n/);
    if (!lines.some(line => /^implicit\s+none\b/i.test(line.replace(/!.*/, "").trim()))) {
      let insertion = 0;
      let continuing = false;
      for (; insertion < lines.length; insertion++) {
        const code = lines[insertion].replace(/!.*/, "").trim();
        if (!code) continue;
        if (continuing) {
          continuing = code.endsWith("&");
          continue;
        }
        if (/^(use|import)\b/i.test(code)) {
          continuing = code.endsWith("&");
          continue;
        }
        if (/^program\s+[a-z]\w*\s*$/i.test(code)) continue;
        break;
      }
      lines.splice(insertion, 0, "implicit none");
      source = lines.join("\n");
    }
    const statements = source.split(/\r?\n/).map(line => line.replace(/!.*/, "").trim()).filter(Boolean);
    const last = statements[statements.length - 1] || "";
    if (!/^end(?:\s+program(?:\s+[a-z]\w*)?)?\s*$/i.test(last))
      source = source.trimEnd() + "\nend\n";
  }
  const url = URL.createObjectURL(new Blob([source], {type:"text/plain"}));
  const link = document.createElement("a");
  link.href = url;
  link.download = isRepl() ? "session.f90" : "program.f90";
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
});

function send(payload) {
  if (busy) return;
  replayDraft = payload.type === "repl-replay" ? payload.source : null;
  busy = true;
  byId("status").textContent = worker ? "Running..." : "Loading interpreter...";
  for (const id of ["run", "repl-submit", "mode", "clear-session"]) byId(id).disabled = true;
  updateSourceControls();
  byId("stop").disabled = false;
  lastRun = {source:sourceText(),
    build:lastRun ? lastRun.build : "Unavailable (interpreter not loaded)", status:"Running"};
  if (!worker) {
    const current = worker = new Worker("worker.mjs", {type:"module"});
    current.onmessage = ({data}) => {
      if (worker !== current) return;
      if (data.type === "status") {
        byId("status").textContent = data.text;
        if (data.build) lastRun.build = data.build;
        if (data.capabilities) { capabilities = {...capabilities, ...data.capabilities}; updateSourceControls(); }
        if (data.available) byId("runtime-notice").hidden = true;
      } else if (data.type === "done") {
        if (data.repl) {
          if (typeof data.autoDeclare === "boolean") byId("auto-declare").checked = data.autoDeclare;
          blockEnds = data.suggestedEnds || [];
          if (replayDraft !== null) {
            const draft = replayDraft;
            replayDraft = null;
            setSource(draft);
            if (!data.ok || data.pending) {
              sessionBaseline = data.source;
              replayNeedsCorrection = true;
              updateSourceControls();
            }
          } else {
            setSource(data.source);
          }
          byId("variables").textContent = data.variables || "(no variables)";
          appendText("output", data.output);
          byId("errors").textContent = data.errors || "";
          byId("repl-hint").textContent = data.pending ? (data.waiting === "continuation" ? "Waiting for a continued line." : "Waiting for more code to complete the block.") : "Enter submits; Shift+Enter adds a line.";
          lastRun.source = data.source;
        } else {
          byId("output").textContent = data.output;
          byId("errors").textContent = data.errors;
        }
        lastRun.status = data.ok ? "Finished" : "Failed";
        finish(`${data.pending ? "Waiting for more code" : data.ok ? "Ready" : "Failed"} / ${data.seconds.toFixed(3)} s`, data.repl);
        if (data.repl) byId("repl-command").focus();
      } else if (data.type === "error") {
        appendText("errors", data.text);
        lastRun.status = "Unable to run";
        finish("Unable to run");
        if (isRepl()) byId("variables").textContent = "(session unavailable; Clear session to restart)";
      }
    };
    current.onerror = event => {
      if (worker !== current) return;
      appendText("errors", event.message || "Unable to load the browser interpreter worker. Serve this directory over HTTP.");
      lastRun.status = "Unable to run";
      finish("Unable to run");
    };
  }
  worker.postMessage({...payload, input:byId("stdin").value,
    autoDeclare:isRepl() && capabilities.autoDeclare && byId("auto-declare").checked});
  timer = setTimeout(() => {
    lastRun.status = "Time limit reached";
    appendText("errors", "Stopped after 30 seconds. Partial output is unavailable; any REPL state and temporary files were discarded.");
    finish("Time limit reached");
    if (isRepl()) {
      setSource("");
      byId("variables").textContent = "(no variables)";
    }
  }, 30000);
}

byId("mode").addEventListener("change", () => {
  if (sourceDirty && !confirm("Discard the unapplied session source edits and leave REPL mode?")) {
    byId("mode").value = "repl";
    return;
  }
  const repl = isRepl();
  if (repl) programSource = sourceText();
  finish("Ready");
  document.body.classList.toggle("repl-mode", repl);
  for (const id of ["repl-entry", "variables-panel", "clear-session"]) byId(id).hidden = !repl;
  byId("clear-code").hidden = repl;
  byId("example").hidden = repl;
  byId("source-heading").textContent = repl ? "01 / SESSION SOURCE" : "01 / SOURCE";
  byId("source-description").textContent = repl ? "Persistent interpreter" : "Free-form Fortran";
  byId("run").innerHTML = repl ? 'Restart and run source <span>Ctrl + Enter</span>' : 'Run program <span>Ctrl + Enter</span>';
  setSource(repl ? "" : programSource);
  byId("output").textContent = "";
  byId("errors").textContent = "";
  byId("variables").textContent = "(no variables)";
  byId("repl-command").value = "";
  commandHistory = [];
  historyIndex = 0;
  if (editor) editor.refresh();
  if (repl) send({type:"repl-init"});
});

byId("repl-submit").addEventListener("click", () => {
  if (busy || sourceDirty) return;
  const command = byId("repl-command").value;
  if (!command.trim()) return;
  commandHistory.push(command);
  historyIndex = commandHistory.length;
  byId("repl-command").value = "";
  send({type:"repl-submit", command});
});

byId("repl-command").addEventListener("keydown", event => {
  if (event.key === "Enter" && event.shiftKey && !event.isComposing) {
    event.preventDefault();
    indentNewline(event.target);
  } else if (event.key === "Tab") {
    event.preventDefault();
    event.target.setRangeText("   ",event.target.selectionStart,event.target.selectionEnd,"end");
  } else if (event.key === "Enter" && !event.shiftKey && !event.isComposing) {
    event.preventDefault();
    byId("repl-submit").click();
  } else if ((event.key === "ArrowUp" || event.key === "ArrowDown") && !event.target.value.includes("\n")) {
    event.preventDefault();
    historyIndex = Math.max(0, Math.min(commandHistory.length, historyIndex + (event.key === "ArrowUp" ? -1 : 1)));
    event.target.value = commandHistory[historyIndex] || "";
  }
});

function indentNewline(input) {
  const before=input.value.slice(0,input.selectionStart);
  const next=input.value.slice(input.selectionEnd).split(/\r?\n/)[0];
  input.setRangeText("\n"+RfortIndent.after(before,next),input.selectionStart,input.selectionEnd,"end");
}

byId("clear-session").addEventListener("click", () => {
  if (busy) return;
  if (sourceDirty && !confirm("Discard the source edits and clear the session?")) return;
  finish("Session cleared");
  setSource("");
  byId("output").textContent = "";
  byId("errors").textContent = "";
  byId("variables").textContent = "(no variables)";
  byId("repl-command").value = "";
  byId("repl-hint").textContent = "Enter submits; Shift+Enter adds a line.";
  commandHistory = [];
  historyIndex = 0;
  send({type:"repl-init"});
});

function reportBlock(text, language = "text") {
  const runs = text.match(/`+/g) || [];
  const fence = "`".repeat(Math.max(3, ...runs.map(run => run.length + 1)));
  return `${fence}${language}\n${text}\n${fence}`;
}
function reportSourceSection(included) {
  const source = lastRun ? lastRun.source : sourceText();
  return "## Fortran source\n" + (included ? reportBlock(source, "fortran") : "Not included. Please provide a minimal reproducer if possible.");
}
function reportBody() {
  const snapshot = lastRun || {source:sourceText(), build:"Unavailable (no program run)", status:"Not run"};
  const parts = [
    "## Problem\nDescribe what went wrong and the steps to reproduce it.",
    "## Expected behavior\nDescribe the expected output or behavior.",
    `## Environment\nBuild: ${snapshot.build}\nRun status: ${snapshot.status}\nBrowser: ${navigator.userAgent}`,
    reportSourceSection(byId("report-source").checked),
    "## Output\n" + reportBlock(byId("output").textContent || "(none)"),
    "## Warnings and errors\n" + reportBlock(byId("errors").textContent || "(none)"),
    "Standard input is not included automatically. Add any non-sensitive input needed to reproduce the problem."
  ];
  return parts.join("\n\n") + "\n";
}
const issueBase = "https://github.com/Beliavsky/rfortran/issues/new";
function issueURL(body) {
  return issueBase + "?" + new URLSearchParams({title:byId("report-title").value.trim() || "Browser playground bug", body});
}
function updateReportNote() {
  const large = issueURL(byId("report-preview").value).length > 7000;
  byId("report-open").textContent = large ? "Open blank GitHub issue" : "Open GitHub issue";
  byId("report-note").textContent = large
    ? "This report is too large for a prefilled URL. Download it, open a blank issue, and paste or attach the report."
    : "Nothing is sent until you open GitHub. You will need a GitHub account to submit the issue.";
}
byId("report").addEventListener("click", () => {
  if (busy) {
    alert("Wait for the run to finish, or press Stop before reporting a bug.");
    return;
  }
  byId("report-source").checked = false;
  byId("report-preview").value = reportBody();
  updateReportNote();
  byId("report-dialog").showModal();
});
byId("report-source").addEventListener("change", () => {
  const included = byId("report-source").checked;
  const previous = reportSourceSection(!included);
  const preview = byId("report-preview");
  if (!preview.value.includes(previous)) {
    byId("report-source").checked = !included;
    byId("report-note").textContent = "The source section has been edited. Add or remove source manually in the preview; your edits have been preserved.";
    return;
  }
  preview.value = preview.value.replace(previous, () => reportSourceSection(included));
  updateReportNote();
});
byId("report-preview").addEventListener("input", updateReportNote);
byId("report-title").addEventListener("input", updateReportNote);
byId("report-open").addEventListener("click", () => {
  const url = issueURL(byId("report-preview").value);
  window.open(url.length > 7000 ? issueBase : url, "_blank", "noopener,noreferrer");
});
byId("report-download").addEventListener("click", () => {
  const text = "# " + byId("report-title").value + "\n\n" + byId("report-preview").value;
  const url = URL.createObjectURL(new Blob([text], {type:"text/markdown;charset=utf-8"}));
  const link = document.createElement("a");
  link.href = url;
  link.download = "rfortran-bug-report.md";
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
});
byId("report-close").addEventListener("click", () => byId("report-dialog").close());

updateSourceControls();
