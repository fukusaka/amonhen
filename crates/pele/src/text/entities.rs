//! 文字の符号（pele の `src/text/entities.ts`）。Mermaid の `#35;`・`#quot;`（`encode_entities` が隠したもの）と
//! HTML の文字参照を文字に戻す。

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::js;

const NAMED: &str = concat!(
    "amp:38,lt:60,gt:62,quot:34,apos:39,nbsp:160,shy:173,iexcl:161,cent:162,pound:163,curren:164,yen:165,sect:167,",
    "copy:169,laquo:171,not:172,reg:174,deg:176,plusmn:177,sup2:178,sup3:179,micro:181,para:182,middot:183,",
    "sup1:185,raquo:187,frac14:188,frac12:189,frac34:190,iquest:191,times:215,divide:247,ndash:8211,mdash:8212,",
    "lsquo:8216,rsquo:8217,sbquo:8218,ldquo:8220,rdquo:8221,bdquo:8222,dagger:8224,Dagger:8225,bull:8226,",
    "hellip:8230,permil:8240,prime:8242,Prime:8243,lsaquo:8249,rsaquo:8250,euro:8364,trade:8482,larr:8592,",
    "uarr:8593,rarr:8594,darr:8595,harr:8596,lArr:8656,uArr:8657,rArr:8658,dArr:8659,hArr:8660,forall:8704,",
    "part:8706,exist:8707,empty:8709,nabla:8711,isin:8712,notin:8713,ni:8715,prod:8719,sum:8721,minus:8722,",
    "radic:8730,infin:8734,and:8743,or:8744,cap:8745,cup:8746,int:8747,there4:8756,sim:8764,cong:8773,",
    "asymp:8776,ne:8800,equiv:8801,le:8804,ge:8805,sub:8834,sup:8835,sube:8838,supe:8839,oplus:8853,otimes:8855,",
    "perp:8869,sdot:8901,loz:9674,spades:9824,clubs:9827,hearts:9829,diams:9830,check:10003,cross:10007,",
    "AMP:38,LT:60,GT:62,QUOT:34,lsqb:91,rsqb:93,lcub:123,rcub:125,verbar:124,VerticalLine:124,midast:42,",
    "DiacriticalGrave:96,Hat:94,UnderBar:95,",
    "equals:61,colon:58,semi:59,num:35,percnt:37,lpar:40,rpar:41,lbrack:91,rbrack:93,lbrace:123,rbrace:125,",
    "vert:124,sol:47,bsol:92,quest:63,excl:33,ast:42,plus:43,comma:44,period:46,dollar:36,commat:64,grave:96,",
    "hat:94,lowbar:95,tilde:126,Alpha:913,Beta:914,Gamma:915,Delta:916,Epsilon:917,Zeta:918,Eta:919,Theta:920,",
    "Iota:921,Kappa:922,Lambda:923,Mu:924,Nu:925,Xi:926,Omicron:927,Pi:928,Rho:929,Sigma:931,Tau:932,",
    "Upsilon:933,Phi:934,Chi:935,Psi:936,Omega:937,alpha:945,beta:946,gamma:947,delta:948,epsilon:949,zeta:950,",
    "eta:951,theta:952,iota:953,kappa:954,lambda:955,mu:956,nu:957,xi:958,omicron:959,pi:960,rho:961,sigmaf:962,",
    "sigma:963,tau:964,upsilon:965,phi:966,chi:967,psi:968,omega:969"
);

fn lookup(name: &str) -> Option<char> {
    static NAMES: OnceLock<HashMap<&'static str, char>> = OnceLock::new();
    NAMES
        .get_or_init(|| {
            NAMED
                .split(',')
                .map(|pair| {
                    let (n, c) = pair.split_once(':').unwrap();
                    (n, char::from_u32(c.parse().unwrap()).unwrap())
                })
                .collect()
        })
        .get(name)
        .copied()
}

fn code_point(n: f64) -> Option<char> {
    if n > 0.0 && n <= 0x10ffff as f64 && !(0xd800 as f64..=0xdfff as f64).contains(&n) {
        char::from_u32(n as u32)
    } else {
        None
    }
}

const OPEN: &str = "ﬂ°";
const CLOSE: &str = "¶ß";

/// Mermaid の置き字は `ﬂ°` から `¶ß` まで、数の符号は 2 つ目の `°` で表す。Mermaid と同じく文字参照に戻し、
/// どの符号も 1 度だけ戻るようにする。
fn restore_placeholders(text: &str) -> String {
    let mut out = String::new();
    let mut last = 0;
    let mut from = text.find(OPEN);
    while let Some(f) = from {
        // `from + 2`（UTF-16）は OPEN の後ろ。
        let after = f + OPEN.len();
        let Some(stop) = text[after..].find('¶').map(|k| k + after) else {
            break;
        };
        if text[stop + '¶'.len_utf8()..].starts_with('ß') {
            let numeric = text[after..].starts_with('°');
            out.push_str(&text[last..f]);
            out.push_str(if numeric { "&#" } else { "&" });
            out.push_str(
                &text[if numeric {
                    after + '°'.len_utf8()
                } else {
                    after
                }..stop],
            );
            out.push(';');
            last = stop + CLOSE.len();
        }
        let next = stop + '¶'.len_utf8();
        from = text[next..].find(OPEN).map(|k| k + next);
    }
    if last == 0 {
        text.to_string()
    } else {
        out + &text[last..]
    }
}

fn word_len(b: &[u8], from: usize) -> usize {
    b[from..]
        .iter()
        .take_while(|c| c.is_ascii_alphanumeric() || **c == b'_')
        .count()
}

/// 知らせの中の置き字を、書いたときの `#name;` に戻す（`/ﬂ°°?(\w+)¶ß/g` → `#$1;`）。
pub fn written_codes(text: &str) -> String {
    if !text.contains(CLOSE) {
        return text.to_string();
    }
    let mut out = String::new();
    let mut last = 0;
    let mut i = 0;
    while let Some(k) = text[i..].find(OPEN) {
        let f = i + k;
        let mut p = f + OPEN.len();
        if text[p..].starts_with('°') {
            p += '°'.len_utf8();
        }
        let n = word_len(text.as_bytes(), p);
        if n > 0 && text[p + n..].starts_with(CLOSE) {
            out.push_str(&text[last..f]);
            out.push('#');
            out.push_str(&text[p..p + n]);
            out.push(';');
            last = p + n + CLOSE.len();
            i = last;
        } else {
            i = f + OPEN.len();
        }
    }
    out.push_str(&text[last..]);
    out
}

/// Mermaid の `#35;`・`#quot;` の符号と HTML の文字参照を文字に。
pub fn decode_entities(text: &str) -> String {
    let mut text = if text.contains(CLOSE) {
        restore_placeholders(text)
    } else {
        text.to_string()
    };
    if text.contains('&') {
        // `/&(?:#(\d+)|#[xX]([0-9a-fA-F]+)|(\w+));/g`
        let b = text.as_bytes();
        let mut out = String::with_capacity(text.len());
        let mut last = 0;
        let mut i = 0;
        while i < b.len() {
            if b[i] != b'&' {
                i += 1;
                continue;
            }
            let mut decoded: Option<(usize, Option<char>)> = None;
            if b.get(i + 1) == Some(&b'#') {
                let d = b[i + 2..].iter().take_while(|c| c.is_ascii_digit()).count();
                if d > 0 && b.get(i + 2 + d) == Some(&b';') {
                    decoded = Some((i + 3 + d, code_point(js::number(&text[i + 2..i + 2 + d]))));
                } else if matches!(b.get(i + 2), Some(b'x' | b'X')) {
                    let h = b[i + 3..]
                        .iter()
                        .take_while(|c| c.is_ascii_hexdigit())
                        .count();
                    if h > 0 && b.get(i + 3 + h) == Some(&b';') {
                        decoded = Some((
                            i + 4 + h,
                            code_point(js::parse_int(&text[i + 3..i + 3 + h], 16)),
                        ));
                    }
                }
            } else {
                let n = word_len(b, i + 1);
                if n > 0 && b.get(i + 1 + n) == Some(&b';') {
                    decoded = Some((i + 2 + n, lookup(&text[i + 1..i + 1 + n])));
                }
            }
            match decoded {
                Some((end, Some(c))) => {
                    out.push_str(&text[last..i]);
                    out.push(c);
                    last = end;
                    i = end;
                }
                Some((end, None)) => i = end,
                None => i += 1,
            }
        }
        out.push_str(&text[last..]);
        text = out;
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes() {
        assert_eq!(
            decode_entities("a &amp; b &#35; &#x41; &nope;"),
            "a & b # A &nope;"
        );
        assert_eq!(decode_entities("ﬂ°quot¶ßxﬂ°°35¶ß"), "\"x#");
        assert_eq!(written_codes("ﬂ°quot¶ß ﬂ°°35¶ß"), "#quot; #35;");
    }
}
