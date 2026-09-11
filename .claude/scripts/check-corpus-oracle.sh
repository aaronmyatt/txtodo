#!/usr/bin/env bash
# Tier-3 check for corpus/*.tokens.json: every entry's spans are sorted, contiguous, start at 0, end at
# byteLength(raw), use only schema kinds, and `raw` equals the matching line of the sibling .txt
# (bytes, BOM stripped, trailing newline is not a blank entry). Exits 1 on any mismatch.
# M1 replaces this with txtodo-core's real round-trip + tokenize test; until then `just corpus` runs it.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
exec node -e '
const fs=require("fs"),path=require("path"); const dir=process.argv[1];
const kinds=new Set(JSON.parse(fs.readFileSync(path.join(dir,"tokens.schema.json"))).items.properties.spans.items.properties.kind.enum);
let bad=0, files=0, lines=0;
for (const f of fs.readdirSync(dir).filter(f=>f.endsWith(".tokens.json"))) {
  files++; const txt=path.join(dir,f.replace(/\.tokens\.json$/,".txt"));
  let s=fs.readFileSync(txt,"utf8"); if(s.charCodeAt(0)===0xFEFF) s=s.slice(1);
  const actual=s.split(/\r?\n/); if(actual.at(-1)===""&&/\n$/.test(s)) actual.pop();
  const oracle=JSON.parse(fs.readFileSync(path.join(dir,f)));
  const err=(i,m)=>{console.log(`${f}:${i+1}: ${m}`); bad++;};
  if(oracle.length!==actual.length) err(-1,`${oracle.length} entries vs ${actual.length} lines in ${path.basename(txt)}`);
  oracle.forEach((e,i)=>{ lines++; if(e.raw!==actual[i]) return err(i,"raw differs from corpus line");
    let pos=0; for(const sp of e.spans){ if(!kinds.has(sp.kind)) err(i,`unknown kind ${sp.kind}`);
      if(sp.start!==pos) err(i,`gap or overlap at byte ${pos} (span starts ${sp.start})`); if(sp.end<=sp.start) err(i,"empty span"); pos=sp.end; }
    if(pos!==Buffer.byteLength(e.raw)) err(i,`spans end at ${pos}, line is ${Buffer.byteLength(e.raw)} bytes`); });
}
console.log(`corpus-oracle: ${files} files, ${lines} lines, ${bad} problems`); process.exit(bad?1:0);
' "$ROOT/corpus"
