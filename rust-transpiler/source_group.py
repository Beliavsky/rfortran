"""Ordered source inputs and diagnostics for one Rust translation unit."""
from bisect import bisect_right
from pathlib import Path
import re

from compiler_frontend import FrontendError


class SourceGroup:
    def __init__(self, parts):
        self.parts, self.starts = [], []
        line = 1
        for name, text in parts:
            # A file without a trailing newline must not swallow the next header.
            text = text if text.endswith("\n") else text+"\n"
            self.parts.append((str(name),text))
            self.starts.append(line)
            line += text.count("\n")
        if not self.parts:
            raise FrontendError("at least one source file is required")
        self.source = "".join(text for _,text in self.parts)

    @classmethod
    def read(cls, paths):
        resolved = [Path(path).resolve() for path in paths]
        if len(set(resolved)) != len(resolved):
            raise FrontendError("duplicate source files in translation group")
        return cls([(path,Path(path).read_text(encoding="utf-8-sig")) for path in paths])

    def locate(self, line):
        index = max(0,bisect_right(self.starts,line)-1)
        return self.parts[index][0],line-self.starts[index]+1

    def label(self, line):
        name,local = self.locate(line)
        return f"{name}:{local}"

    def diagnostics(self, text, snapshot):
        text = re.sub(re.escape(str(snapshot))+r":(\d+):(\d+)",
                      lambda match:self.label(int(match[1]))+":"+match[2],text)
        # Also translate line numbers on the compiler's displayed source lines.
        text = re.sub(r"(?m)^(\s*)(\d+)(\s*[│|])",
                      lambda match:match[1]+str(self.locate(int(match[2]))[1]).rjust(len(match[2]))+match[3],text)
        return text.replace(str(snapshot),"<source group>")
