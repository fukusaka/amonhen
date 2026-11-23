#!/usr/bin/env python3
# ColorBrewer の配色の表（crates/dot/src/brewer.rs）を作る。
#   python3 -I scripts/gen-colorbrewer.py <colorbrewer.json> > crates/dot/src/brewer.rs
# <colorbrewer.json> は axismaps/colorbrewer（8ca8dec5f66236bdaa233230c169c8ad5430e52c）の
# export/colorbrewer.json（Apache-2.0 形式の ColorBrewer のライセンス）。
# Graphviz と同じく、配色の名前は小文字の名前と色の数（blues9・set312 など）。
import json
import re
import sys

LICENSE = """Apache-Style Software License for ColorBrewer software and ColorBrewer Color
Schemes

Copyright (c) 2002 Cynthia Brewer, Mark Harrower, and The Pennsylvania State
University.

Licensed under the Apache License, Version 2.0 (the "License"); you may not
use this file except in compliance with the License. You may obtain a copy of
the License at

http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS, WITHOUT
WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied. See the
License for the specific language governing permissions and limitations under
the License.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions as source code must retain the above copyright notice, this
list of conditions and the following disclaimer.

2. The end-user documentation included with the redistribution, if any, must
include the following acknowledgment: "This product includes color
specifications and designs developed by Cynthia Brewer
(http://colorbrewer.org/)." Alternately, this acknowledgment may appear in the
software itself, if and wherever such third-party acknowledgments normally
appear.

4. The name "ColorBrewer" must not be used to endorse or promote products
derived from this software without prior written permission. For written
permission, please contact Cynthia Brewer at cbrewer@psu.edu.

5. Products derived from this software may not be called "ColorBrewer", nor
may "ColorBrewer" appear in their name, without prior written permission of
Cynthia Brewer."""


def main():
    with open(sys.argv[1], encoding="utf-8") as f:
        data = json.load(f)
    schemes = []
    for name, classes in data.items():
        for count, colors in classes.items():
            if not count.isdigit():
                continue
            values = []
            for c in colors:
                m = re.fullmatch(r"rgb\((\d+),\s*(\d+),\s*(\d+)\)", c.strip())
                if not m:
                    raise SystemExit(f"unexpected color {c!r} in {name}{count}")
                r, g, b = (int(v) for v in m.groups())
                values.append(f"0x{r:02x}{g:02x}{b:02x}")
            if len(values) != int(count):
                raise SystemExit(f"{name}{count} has {len(values)} colors")
            schemes.append((f"{name.lower()}{count}", values))
    schemes.sort()
    out = [
        "//! ColorBrewer の配色（Graphviz の `colorscheme` の名前: 小文字の名前と色の数）。生成したもの: 手で直さない",
        "//! （`scripts/gen-colorbrewer.py`。元は axismaps/colorbrewer 8ca8dec5 の `export/colorbrewer.json`）。",
        "",
        "// ColorBrewer のライセンス:",
        "//",
    ]
    out += ["//" + ("" if not line else " " + line) for line in LICENSE.splitlines()]
    out += [
        "",
        "/// 名前の順に並べた（二分探索する）配色の名前と、1 番からの色（0xRRGGBB）。",
        "#[rustfmt::skip]",
        "pub static BREWER: &[(&str, &[u32])] = &[",
    ]
    for name, values in schemes:
        out.append(f'    ("{name}", &[{", ".join(values)}]),')
    out.append("];")
    print("\n".join(out))


main()
