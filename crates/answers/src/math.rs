//! Maths: "2^10", "sqrt(144)", "(3+4)*5", "15% of 80", "80 + 15%", "3 x 4".
use crate::{num, plain};
use ss_core::Answer;

pub fn answer(q: &str) -> Option<Answer> {
    let mut s = format!(" {q} ");
    for (a, b) in [
        (" plus ", " + "), (" minus ", " - "), (" times ", " * "), (" multiplied by ", " * "), (" x ", " * "),
        (" divided by ", " / "), (" over ", " / "), (" to the power of ", " ^ "),
        (" percent ", "% "), (" squared ", " ^ 2 "), (" cubed ", " ^ 3 "), ("square root of ", "sqrt "), ("cube root of ", "cbrt "),
        ("°", " deg "),
    ] {
        s = s.replace(a, b);
    }
    let s = s.trim();
    // Dates and phone numbers are not sums: 2026-10-18, 18/10/2026, 2026-10, 555-1234.
    let dashed = s.split(['-', '/']).count();
    let long = s.contains('-') && s.split('-').any(|g| g.len() >= 4);
    if s.bytes().all(|c| c.is_ascii_digit() || b"-/.".contains(&c)) && (dashed > 2 || long) {
        return None;
    }
    let mut p = Parser { s, i: 0, ops: 0, percent: false };
    let v = p.expr()?;
    p.ws();
    if p.i != s.len() || p.ops == 0 || !v.v.is_finite() {
        return None;
    }
    let output = if v.pct { format!("{}%", num(v.v * 100.0, 10)) } else { num(v.v, 10) };
    Some(Answer {
        kind: if p.percent { "percent" } else { "math" }.into(),
        input: q.into(),
        input_label: String::new(),
        copy: plain(&output),
        output,
        output_label: String::new(),
        detail: String::new(),
    })
}

#[derive(Clone, Copy)]
struct V {
    v: f64,
    /// A percentage, so "80 + 15%" means 80 plus 15% of 80.
    pct: bool,
}

struct Parser<'a> {
    s: &'a str,
    i: usize,
    /// Operators and functions seen; a bare number is not an answer.
    ops: u32,
    percent: bool,
}

type Func = (&'static str, fn(f64) -> f64);
const FUNCS: &[Func] = &[
    ("sqrt", f64::sqrt), ("cbrt", f64::cbrt), ("abs", f64::abs), ("sin", f64::sin), ("cos", f64::cos),
    ("tan", f64::tan), ("asin", f64::asin), ("acos", f64::acos), ("atan", f64::atan), ("ln", f64::ln),
    ("log", f64::log10), ("log10", f64::log10), ("log2", f64::log2), ("exp", f64::exp), ("round", f64::round),
    ("floor", f64::floor), ("ceil", f64::ceil),
];
const CONSTS: &[(&str, f64)] = &[("pi", std::f64::consts::PI), ("tau", std::f64::consts::TAU), ("e", std::f64::consts::E)];

impl Parser<'_> {
    fn rest(&self) -> &str {
        &self.s[self.i..]
    }

    fn ws(&mut self) {
        self.i = self.s.len() - self.rest().trim_start().len();
    }

    fn eat(&mut self, t: &str) -> bool {
        self.ws();
        let ok = self.rest().starts_with(t);
        if ok {
            self.i += t.len();
        }
        ok
    }

    /// The letters (then digits, for log10) at the cursor.
    fn word(&mut self) -> &str {
        self.ws();
        let r = self.rest();
        let n = r.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(r.len());
        let m = n + r[n..].find(|c: char| !c.is_ascii_digit()).unwrap_or(r.len() - n);
        if n > 0 && FUNCS.iter().any(|f| f.0 == &r[..m]) { &r[..m] } else { &r[..n] }
    }

    fn eat_word(&mut self, w: &str) -> bool {
        self.word() == w && self.eat(w)
    }

    fn expr(&mut self) -> Option<V> {
        let mut a = self.term()?;
        loop {
            let sign = if self.eat("+") { 1.0 } else if self.eat("-") { -1.0 } else { return Some(a) };
            let b = self.term()?;
            self.ops += 1;
            a = if b.pct && !a.pct {
                V { v: a.v * (1.0 + sign * b.v), pct: false }
            } else {
                V { v: a.v + sign * b.v, pct: a.pct && b.pct }
            };
        }
    }

    fn term(&mut self) -> Option<V> {
        let mut a = self.unary()?;
        loop {
            let glued_x = self.rest().starts_with('x') && self.rest()[1..].starts_with(|c: char| c.is_ascii_digit());
            let op = if self.eat("*") || (glued_x && self.eat("x")) {
                '*'
            } else if self.eat("/") {
                '/'
            } else if self.eat("%") || self.eat_word("mod") {
                '%'
            } else if a.pct && self.eat_word("of") {
                'o'
            } else if a.pct && self.eat_word("off") {
                'f'
            } else if self.rest().starts_with('(') || { let w = self.word(); FUNCS.iter().any(|f| f.0 == w) || CONSTS.iter().any(|c| c.0 == w) } {
                '*'
            } else {
                return Some(a);
            };
            let b = self.unary()?;
            self.ops += 1;
            let v = match op {
                '*' | 'o' => a.v * b.v,
                '/' => a.v / b.v,
                '%' => a.v % b.v,
                _ => b.v * (1.0 - a.v),
            };
            a = V { v, pct: false };
        }
    }

    fn unary(&mut self) -> Option<V> {
        if self.eat("-") {
            let v = self.unary()?;
            return Some(V { v: -v.v, ..v });
        }
        if self.eat("+") {
            return self.unary();
        }
        let a = self.postfix()?;
        if self.eat("^") || self.eat("**") {
            let b = self.unary()?;
            self.ops += 1;
            return Some(V { v: a.v.powf(b.v), pct: false });
        }
        Some(a)
    }

    fn postfix(&mut self) -> Option<V> {
        let mut a = self.primary()?;
        loop {
            self.ws();
            let r = self.rest();
            if r.starts_with('!') && !r.starts_with("!=") {
                if a.v.fract() != 0.0 || !(0.0..=170.0).contains(&a.v) {
                    return None;
                }
                self.i += 1;
                self.ops += 1;
                a.v = (1..=a.v as u32).map(f64::from).product();
            } else if r.starts_with('%') && !r[1..].trim_start().starts_with(|c: char| c.is_ascii_digit() || c == '(' || c == '.') {
                // A percent sign, not "50 % 3" (modulo).
                self.i += 1;
                self.percent = true;
                a = V { v: a.v / 100.0, pct: true };
            } else if self.eat_word("deg") {
                a.v = a.v.to_radians();
            } else {
                return Some(a);
            }
        }
    }

    fn primary(&mut self) -> Option<V> {
        if self.eat("(") {
            let v = self.expr()?;
            return self.eat(")").then_some(v);
        }
        self.ws();
        if self.rest().starts_with(|c: char| c.is_ascii_digit() || c == '.') {
            let (mut v, _, rest) = crate::convert::number(self.rest())?;
            self.i = self.s.len() - rest.len();
            for (w, m) in [("k", 1e3), ("bn", 1e9), (" thousand", 1e3), (" million", 1e6), (" billion", 1e9)] {
                let r = self.rest();
                if r.starts_with(w) && !r[w.len()..].starts_with(|c: char| c.is_ascii_alphabetic()) {
                    self.i += w.len();
                    v *= m;
                }
            }
            return Some(V { v, pct: false });
        }
        let w = self.word().to_string();
        if let Some(c) = CONSTS.iter().find(|c| c.0 == w) {
            self.i += w.len();
            return Some(V { v: c.1, pct: false });
        }
        let f = FUNCS.iter().find(|f| f.0 == w)?;
        self.i += w.len();
        let arg = self.unary()?;
        self.ops += 1;
        Some(V { v: (f.1)(arg.v), pct: false })
    }
}
