# pele test data

The `.mmd` files are the examples from pele's test corpus
(`tests/corpus/<type>-docs.json`, which collect the examples in Mermaid's
documentation), one file per example, named after the corpus. `.err` holds the
message when pele refuses the input. The `.svg` files are what pele 1.13.0 renders from them; the
Rust port must produce the same bytes. pele is run with `TZ=UTC`: gantt charts
count in local time, which the port takes to be UTC so that its output does not
depend on the machine.

pele: https://github.com/obsidianmd/pele, MIT License, Copyright (c) 2026 kepano.
Mermaid's documentation: https://github.com/mermaid-js/mermaid, MIT License.
