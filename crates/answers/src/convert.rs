//! Units and money: `5'11" in cm`, "100k idr in usd", "usd to idr", "5 km".
use crate::{fixed, num, plain, Rates};
use ss_core::Answer;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Dim {
    Length,
    Mass,
    Volume,
    Temp,
    Speed,
    Area,
    Data,
    Time,
}
use Dim::*;

/// `base = value * factor + offset`, in m, kg, L, K, m/s, m², bytes or s.
/// `names` are typed forms with spaces removed; plurals are found by dropping
/// a final "s"/"es". A `*` in `sym` means "add s when not 1" (cup → cups).
/// `pair` is the unit a bare quantity converts to ("" = needs a target).
struct Unit {
    sym: &'static str,
    names: &'static str,
    dim: Dim,
    factor: f64,
    offset: f64,
    label: &'static str,
    pair: &'static str,
    note: &'static str,
}

const fn u(sym: &'static str, names: &'static str, dim: Dim, factor: f64, label: &'static str, pair: &'static str) -> Unit {
    Unit { sym, names, dim, factor, offset: 0.0, label, pair, note: "" }
}

#[rustfmt::skip]
const UNITS: &[Unit] = &[
    u("mm", "mm millimetre millimeter", Length, 0.001, "Millimetres", "in"),
    u("cm", "cm centimetre centimeter", Length, 0.01, "Centimetres", "in"),
    u("m", "m metre meter", Length, 1.0, "Metres", "ft"),
    u("km", "km kilometre kilometer", Length, 1000.0, "Kilometres", "mi"),
    u("in", "in inch inches \"", Length, 0.0254, "Inches", "cm"),
    u("ft", "ft foot feet ' feetandinches", Length, 0.3048, "Feet", "m"),
    u("yd", "yd yard", Length, 0.9144, "Yards", "m"),
    u("mi", "mi mile", Length, 1609.344, "Miles", "km"),
    u("nmi", "nmi nauticalmile", Length, 1852.0, "Nautical miles", "km"),
    u("mg", "mg milligram", Mass, 1e-6, "Milligrams", "g"),
    u("g", "g gram gramme", Mass, 0.001, "Grams", "oz"),
    u("kg", "kg kilo kilogram kilogramme", Mass, 1.0, "Kilograms", "lb"),
    Unit { note: "Metric tonne; a US ton is 907.18 kg", ..u("t", "t tonne ton metricton", Mass, 1000.0, "Tonnes", "kg") },
    u("US ton*", "uston shortton", Mass, 907.18474, "US tons", "kg"),
    u("long ton*", "longton ukton imperialton", Mass, 1016.0469088, "Long tons", "kg"),
    u("lb", "lb pound", Mass, 0.45359237, "Pounds", "kg"),
    u("oz", "oz ounce", Mass, 0.028349523125, "Ounces", "g"),
    u("st", "stone", Mass, 6.35029318, "Stone", "kg"),
    u("mL", "ml millilitre milliliter cc", Volume, 0.001, "Millilitres", "fl oz"),
    u("L", "l litre liter", Volume, 1.0, "Litres", "gal"),
    Unit { note: "US teaspoon; a metric one is 5 mL", ..u("tsp", "tsp teaspoon", Volume, 0.00492892159375, "US teaspoons", "mL") },
    Unit { note: "US tablespoon; Australian is 20 mL, UK 15 mL", ..u("tbsp", "tbsp tbs tablespoon", Volume, 0.01478676478125, "US tablespoons", "mL") },
    Unit { note: "US fluid ounce; UK is 28.41 mL", ..u("fl oz", "floz fluidounce", Volume, 0.0295735295625, "US fluid ounces", "mL") },
    Unit { note: "US cup; a metric cup is 250 mL", ..u("cup*", "cup", Volume, 0.2365882365, "US cups", "mL") },
    u("metric cup*", "metriccup", Volume, 0.25, "Metric cups", "mL"),
    Unit { note: "US pint; UK is 568 mL", ..u("pt", "pt pint", Volume, 0.473176473, "US pints", "mL") },
    u("UK pt", "ukpint imperialpint", Volume, 0.56826125, "UK pints", "mL"),
    u("qt", "qt quart", Volume, 0.946352946, "US quarts", "L"),
    Unit { note: "US gallon; UK is 4.546 L", ..u("gal", "gal gallon", Volume, 3.785411784, "US gallons", "L") },
    u("UK gal", "ukgal ukgallon imperialgallon", Volume, 4.54609, "UK gallons", "L"),
    u("m³", "m3 m^3 cubicmetre cubicmeter", Volume, 1000.0, "Cubic metres", "L"),
    Unit { offset: 273.15, ..u("°C", "°c c celsius centigrade degc degreec degreesc degreecelsius degreescelsius", Temp, 1.0, "Celsius", "°F") },
    Unit { offset: 273.15 - 32.0 * 5.0 / 9.0, ..u("°F", "°f f fahrenheit degf degreef degreesf degreefahrenheit degreesfahrenheit", Temp, 5.0 / 9.0, "Fahrenheit", "°C") },
    u("K", "k kelvin", Temp, 1.0, "Kelvin", "°C"),
    u("km/h", "km/h kmh kph kmph kmperhour kilometreperhour kilometresperhour kilometerperhour kilometersperhour", Speed, 1.0 / 3.6, "Kilometres per hour", "mph"),
    u("mph", "mph mi/h mileperhour milesperhour", Speed, 0.44704, "Miles per hour", "km/h"),
    u("m/s", "m/s mps metrepersecond metrespersecond meterpersecond meterspersecond", Speed, 1.0, "Metres per second", "km/h"),
    u("kn", "kn kt knot", Speed, 1852.0 / 3600.0, "Knots", "km/h"),
    u("m²", "m2 m^2 sqm sqmetre sqmeter squaremetre squaremeter", Area, 1.0, "Square metres", "ft²"),
    u("km²", "km2 km^2 sqkm squarekilometre squarekilometer", Area, 1e6, "Square kilometres", "mi²"),
    u("cm²", "cm2 cm^2 sqcm squarecentimetre squarecentimeter", Area, 1e-4, "Square centimetres", "in²"),
    u("ha", "ha hectare", Area, 1e4, "Hectares", "ac"),
    u("ac", "ac acre", Area, 4046.8564224, "Acres", "ha"),
    u("ft²", "ft2 ft^2 sqft squarefoot squarefeet", Area, 0.09290304, "Square feet", "m²"),
    u("in²", "in2 in^2 sqin squareinch", Area, 0.00064516, "Square inches", "cm²"),
    u("mi²", "mi2 mi^2 sqmi squaremile", Area, 2589988.110336, "Square miles", "km²"),
    u("yd²", "yd2 yd^2 sqyd squareyard", Area, 0.83612736, "Square yards", "m²"),
    u("bit*", "bit", Data, 0.125, "Bits", ""),
    u("B", "b byte", Data, 1.0, "Bytes", ""),
    u("KB", "kb kilobyte", Data, 1e3, "Kilobytes", ""),
    u("MB", "mb megabyte", Data, 1e6, "Megabytes", ""),
    u("GB", "gb gigabyte", Data, 1e9, "Gigabytes", ""),
    u("TB", "tb terabyte", Data, 1e12, "Terabytes", ""),
    u("PB", "pb petabyte", Data, 1e15, "Petabytes", ""),
    u("KiB", "kib kibibyte", Data, 1024.0, "Kibibytes", ""),
    u("MiB", "mib mebibyte", Data, 1048576.0, "Mebibytes", ""),
    u("GiB", "gib gibibyte", Data, 1073741824.0, "Gibibytes", ""),
    u("TiB", "tib tebibyte", Data, 1099511627776.0, "Tebibytes", ""),
    u("ms", "ms millisecond msec", Time, 0.001, "Milliseconds", ""),
    u("s", "s sec second", Time, 1.0, "Seconds", ""),
    u("min", "min minute", Time, 60.0, "Minutes", ""),
    u("h", "h hr hour", Time, 3600.0, "Hours", ""),
    u("day*", "d day", Time, 86400.0, "Days", ""),
    u("week*", "wk week", Time, 604800.0, "Weeks", ""),
    u("fortnight*", "fortnight", Time, 1209600.0, "Fortnights", ""),
    Unit { note: "An average month, 30.44 days", ..u("month*", "mo month", Time, 2629746.0, "Months", "") },
    Unit { note: "An average year, 365.24 days", ..u("year*", "yr year", Time, 31556952.0, "Years", "") },
];

/// ECB currencies: code, label, typed names (spaces removed).
#[rustfmt::skip]
const MONEY: &[(&str, &str, &str)] = &[
    ("AUD", "Australian dollars", "aud a$ au$ aussiedollar australiandollar"),
    ("BRL", "Brazilian reais", "brl r$ real reais brazilianreal"),
    ("CAD", "Canadian dollars", "cad c$ ca$ canadiandollar"),
    ("CHF", "Swiss francs", "chf franc swissfranc"),
    ("CNY", "Chinese yuan", "cny yuan rmb renminbi chineseyuan"),
    ("CZK", "Czech koruna", "czk koruna"),
    ("DKK", "Danish kroner", "dkk danishkrone"),
    ("EUR", "Euros", "eur € euro"),
    ("GBP", "British pounds", "gbp £ pound quid sterling poundsterling britishpound"),
    ("HKD", "Hong Kong dollars", "hkd hk$ hongkongdollar"),
    ("HUF", "Hungarian forints", "huf forint"),
    ("IDR", "Indonesian rupiah", "idr rp rupiah indonesianrupiah"),
    ("ILS", "Israeli shekels", "ils ₪ shekel"),
    ("INR", "Indian rupees", "inr ₹ rupee indianrupee"),
    ("ISK", "Icelandic krónur", "isk"),
    ("JPY", "Japanese yen", "jpy ¥ yen japaneseyen"),
    ("KRW", "South Korean won", "krw ₩ won koreanwon southkoreanwon"),
    ("MXN", "Mexican pesos", "mxn peso mexicanpeso"),
    ("MYR", "Malaysian ringgit", "myr rm ringgit malaysianringgit"),
    ("NOK", "Norwegian kroner", "nok norwegiankrone"),
    ("NZD", "New Zealand dollars", "nzd nz$ kiwidollar newzealanddollar"),
    ("PHP", "Philippine pesos", "php ₱ philippinepeso"),
    ("PLN", "Polish złoty", "pln zł zloty"),
    ("RON", "Romanian lei", "ron leu lei"),
    ("SEK", "Swedish kronor", "sek swedishkrona"),
    ("SGD", "Singapore dollars", "sgd s$ sg$ singaporedollar"),
    ("THB", "Thai baht", "thb ฿ baht thaibaht"),
    ("TRY", "Turkish lira", "try lira turkishlira"),
    ("USD", "US dollars", "usd $ us$ dollar buck usdollar americandollar"),
    ("ZAR", "South African rand", "zar rand"),
];

/// Home currency by time zone prefix; the first match wins.
#[rustfmt::skip]
const HOME: &[(&str, &str)] = &[
    ("Australia/", "AUD"), ("Pacific/Auckland", "NZD"), ("Asia/Jakarta", "IDR"), ("Asia/Makassar", "IDR"),
    ("Asia/Jayapura", "IDR"), ("Asia/Pontianak", "IDR"), ("Asia/Singapore", "SGD"), ("Asia/Kuala_Lumpur", "MYR"),
    ("Asia/Kuching", "MYR"), ("Asia/Bangkok", "THB"), ("Asia/Tokyo", "JPY"), ("Asia/Seoul", "KRW"),
    ("Asia/Shanghai", "CNY"), ("Asia/Hong_Kong", "HKD"), ("Asia/Manila", "PHP"), ("Asia/Kolkata", "INR"),
    ("Asia/Jerusalem", "ILS"), ("Europe/London", "GBP"), ("Europe/Zurich", "CHF"), ("Europe/Stockholm", "SEK"),
    ("Europe/Oslo", "NOK"), ("Europe/Copenhagen", "DKK"), ("Europe/Warsaw", "PLN"), ("Europe/Prague", "CZK"),
    ("Europe/Budapest", "HUF"), ("Europe/Bucharest", "RON"), ("Europe/Istanbul", "TRY"), ("Atlantic/Reykjavik", "ISK"),
    ("Africa/Johannesburg", "ZAR"), ("America/Sao_Paulo", "BRL"), ("America/Mexico_City", "MXN"),
    ("America/Toronto", "CAD"), ("America/Vancouver", "CAD"), ("America/Edmonton", "CAD"), ("America/Winnipeg", "CAD"),
    ("America/Halifax", "CAD"), ("Europe/", "EUR"),
];

/// Number words, longest first.
const WORDS: &[(&str, f64)] = &[
    ("a quarter of a ", 0.25), ("quarter of a ", 0.25), ("a quarter ", 0.25), ("half a ", 0.5), ("half an ", 0.5),
    ("a dozen ", 12.0), ("dozen ", 12.0), ("half ", 0.5), ("quarter ", 0.25), ("a ", 1.0), ("an ", 1.0), ("one ", 1.0), ("two ", 2.0),
    ("three ", 3.0), ("four ", 4.0), ("five ", 5.0), ("six ", 6.0), ("seven ", 7.0), ("eight ", 8.0),
    ("nine ", 9.0), ("ten ", 10.0), ("eleven ", 11.0), ("twelve ", 12.0),
];

/// Multipliers after a number. `tight` ones must touch it ("100k"); `money`
/// ones also need a currency next ("1m idr", but "1m" is a metre).
const MULT: &[(&str, f64, bool, bool)] = &[
    ("thousand", 1e3, false, false), ("million", 1e6, false, false), ("billion", 1e9, false, false),
    ("trillion", 1e12, false, false), ("ribu", 1e3, false, false), ("juta", 1e6, false, false),
    ("dozen", 12.0, false, false), ("k", 1e3, true, false), ("rb", 1e3, true, false), ("jt", 1e6, true, false),
    ("mn", 1e6, true, false), ("mil", 1e6, true, false), ("bn", 1e9, true, false), ("m", 1e6, true, true),
    ("b", 1e9, true, true),
];

fn find_name<T>(key: &str, items: impl Iterator<Item = (T, &'static str)> + Clone) -> Option<T> {
    let tries = [Some(key), key.strip_suffix('s'), key.strip_suffix("es")];
    tries.into_iter().flatten().filter(|k| !k.is_empty()).find_map(|k| {
        items.clone().find(|(_, names)| names.split(' ').any(|n| n == k)).map(|(t, _)| t)
    })
}

fn unit(key: &str) -> Option<&'static Unit> {
    find_name(key, UNITS.iter().map(|u| (u, u.names)))
}

fn money(key: &str) -> Option<&'static str> {
    find_name(key, MONEY.iter().map(|m| (m.0, m.2)))
}

/// Typed unit text as a lookup key: "sq. ft" → "sqft", "km per hour" → "kmperhour".
fn squash(s: &str) -> String {
    s.chars().filter(|c| *c != ' ' && *c != '.').collect()
}

/// A leading number: "1,000", "1.5", "1e3", ".5", "-40". Value, raw text, rest.
pub(crate) fn number(s: &str) -> Option<(f64, &str, &str)> {
    let b = s.as_bytes();
    let mut i = usize::from(matches!(b.first(), Some(b'-' | b'+')));
    while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.' || b[i] == b',') {
        i += 1;
    }
    let digit = |j: usize| b.get(j).is_some_and(u8::is_ascii_digit);
    if b.get(i) == Some(&b'e') && (digit(i + 1) || (matches!(b.get(i + 1), Some(b'-' | b'+')) && digit(i + 2))) {
        i += 2;
        while digit(i) {
            i += 1;
        }
    }
    let raw = s[..i].trim_end_matches([',', '.']);
    let (int, _) = raw.split_once('.').unwrap_or((raw, ""));
    let mut groups = int.trim_start_matches(['-', '+']).split(',');
    let first = groups.next()?;
    if raw.contains(',') && (first.is_empty() || first.len() > 3 || groups.any(|g| g.len() != 3)) {
        return None;
    }
    Some((raw.replace(',', "").parse().ok()?, raw, &s[raw.len()..]))
}

/// A parsed amount: `5'11"`, "100k idr", "$20", "half a mile", "usd".
struct Amount {
    v: f64,
    unit: Option<&'static Unit>,
    money: Option<&'static str>,
    /// Some number was given, in digits or words.
    counted: bool,
    /// The number was written in digits.
    digits: bool,
    /// A one-letter unit glued to its number ("5g"): never a bare answer.
    glued: bool,
    text: String,
    label: String,
}

fn amount(s: &str) -> Option<Amount> {
    let s = s.trim();
    // A currency symbol in front: "$20", "a$50", "rp 100k".
    let sym = MONEY
        .iter()
        .flat_map(|m| m.2.split(' ').map(move |n| (m.0, n)))
        .filter(|(_, n)| n.contains(|c: char| !c.is_ascii_alphabetic()) || *n == "rp" || *n == "rm")
        .filter(|(_, n)| s.strip_prefix(n).is_some_and(|r| r.trim_start().starts_with(|c: char| c.is_ascii_digit())))
        .max_by_key(|(_, n)| n.len());
    let s = sym.map_or(s, |(_, n)| s[n.len()..].trim_start());
    let (mut v, raw, mut rest, digits) = match number(s) {
        // A fraction: "1/2 cup".
        Some((v, raw, rest)) => match rest.strip_prefix('/').and_then(number) {
            Some((d, _, rest)) if d != 0.0 => (v / d, raw, rest, true),
            _ => (v, raw, rest, true),
        },
        None => match WORDS.iter().find(|(w, _)| s.starts_with(w)) {
            Some((w, v)) => (*v, "", &s[w.len()..], false),
            None => (1.0, "", s, false),
        },
    };
    let counted = digits || rest.len() < s.len();
    if digits {
        let word = rest.trim_start().split(|c: char| !c.is_ascii_alphabetic()).next().unwrap_or("");
        let after = rest.trim_start()[word.len()..].trim();
        let tight = !rest.starts_with(' ');
        if let Some(m) = MULT.iter().find(|m| {
            m.0 == word && (tight || !m.2) && (!m.3 || sym.is_some() || money(&squash(after)).is_some())
        }) {
            v *= m.1;
            rest = after;
        }
    }
    let glued = !rest.starts_with(' ');
    let rest = rest.trim();
    let key = squash(rest.strip_suffix(" of water").unwrap_or(rest));
    let money = match sym {
        Some((code, _)) if key.is_empty() => Some(code),
        Some(_) => Some(money(&key)?),
        None => money(&key),
    };
    if money == Some("IDR") && raw.contains('.') && !raw.contains(',') && raw.split('.').skip(1).all(|g| g.len() == 3) {
        // Indonesians write thousands with dots: "Rp 100.000".
        v *= raw.replace('.', "").parse::<f64>().ok()? / raw.parse::<f64>().ok()?;
    }
    // "pound" is both a mass and a currency: keep both, the target decides.
    let unit = if sym.is_some() || key.is_empty() { None } else { unit(&key) };
    if unit.is_none() && money.is_none() && !key.is_empty() {
        return compound(v, rest, digits);
    }
    let glued = glued && digits && unit.is_some() && key.chars().count() == 1;
    let (text, label) = unit.map_or_else(Default::default, |u| (show(v, u), u.label.to_string()));
    Some(Amount { v, unit, money, counted, digits, glued, text, label })
}

/// Two amounts of one kind written together: `5'11"`, "5 ft 11", "1 h 30 min".
fn compound(v: f64, rest: &str, digits: bool) -> Option<Amount> {
    let j = rest.find(|c: char| c.is_ascii_digit()).filter(|_| digits)?;
    let u1 = unit(&squash(&rest[..j]))?;
    let second = amount(&rest[j..])?;
    let inch = UNITS.iter().find(|u| u.sym == "in");
    let u2 = second.unit.or(inch.filter(|_| u1.sym == "ft" && second.money.is_none() && second.digits))?;
    if u2.dim != u1.dim || u2.offset != 0.0 || std::ptr::eq(u1, u2) {
        return None;
    }
    let (text, label) = match second.unit {
        Some(_) => (second.text, second.label),
        None => (show(second.v, u2), u2.label.into()),
    };
    Some(Amount {
        v: v * u1.factor / u2.factor + second.v,
        unit: Some(u2),
        money: None,
        counted: true,
        digits: true,
        glued: false,
        text: format!("{} {text}", show(v, u1)),
        label: format!("{} and {}", u1.label, label.to_lowercase()),
    })
}

fn show(v: f64, u: &Unit) -> String {
    match u.sym.strip_suffix('*') {
        Some(s) if v != 1.0 => format!("{} {s}s", num(v, 6)),
        Some(s) => format!("{} {s}", num(v, 6)),
        None => format!("{} {}", num(v, 6), u.sym),
    }
}

fn label(code: &str) -> &'static str {
    MONEY.iter().find(|m| m.0 == code).map_or("", |m| m.1)
}

pub fn answer(q: &str, rates: Option<&Rates>, home_tz: &str) -> Option<Answer> {
    // "X to Y": try each "to"/"in" from the right, so "12 in to cm" and "1 in in cm" work.
    let cuts = (0..q.len()).rev().flat_map(|i| [" to ", " in ", " into ", " as "].map(|s| (i, s)));
    for (i, sep) in cuts.filter(|(i, s)| q.get(*i..).is_some_and(|r| r.starts_with(s))) {
        let (Some(a), key) = (amount(&q[..i]), squash(&q[i + sep.len()..])) else { continue };
        if let (Some(from), Some(to)) = (a.unit, unit(&key)) {
            let fits = from.dim == to.dim || matches!((from.dim, to.dim), (Volume, Mass) | (Mass, Volume));
            if fits && (a.counted || !std::ptr::eq(from, to)) {
                return units(&a, to);
            }
        }
        if let (Some(from), Some(to)) = (a.money, money(&key))
            && (a.counted || from != to)
        {
            return currency(&a, to, rates?);
        }
    }
    // A bare amount: "5 km", "72 f", "20 bucks".
    let a = amount(q).filter(|a| a.digits && !a.glued)?;
    if let Some(to) = a.unit.and_then(|u| UNITS.iter().find(|t| !u.pair.is_empty() && t.sym.trim_end_matches('*') == u.pair)) {
        return units(&a, to);
    }
    let from = a.money?;
    let home = HOME.iter().find(|h| home_tz.starts_with(h.0)).map_or("USD", |h| h.1);
    let to = if from != home { home } else if home != "USD" { "USD" } else { "EUR" };
    currency(&a, to, rates?)
}

fn units(a: &Amount, to: &'static Unit) -> Option<Answer> {
    let from = a.unit?;
    let r = Some((a.v * from.factor + from.offset - to.offset) / to.factor).filter(|r| r.is_finite())?;
    let output = show(r, to);
    let decimal_data = |u: &Unit| u.dim == Data && u.sym.len() == 2;
    let detail = if from.dim != to.dim {
        "Assuming water (1 mL weighs 1 g); other ingredients differ".to_string()
    } else if !from.note.is_empty() || !to.note.is_empty() {
        if from.note.is_empty() { to.note } else { from.note }.to_string()
    } else if decimal_data(from) && decimal_data(to) {
        let (p, q) = (from.factor.log10() / 3.0, to.factor.log10() / 3.0);
        let bin = |u: &Unit| format!("{}iB", &u.sym[..1]);
        format!("Binary units: {} {} = {} {}", num(a.v, 6), bin(from), num(a.v * 1024f64.powf(p - q), 6), bin(to))
    } else if to.sym == "ft" && from.sym != "ft" && r.abs() < 10.0 {
        let inches = (r * 120.0).round() / 10.0;
        format!("{} ft {} in", (inches / 12.0).floor(), num(inches % 12.0, 3))
    } else {
        String::new()
    };
    Some(Answer {
        kind: "units".into(),
        input: a.text.clone(),
        input_label: a.label.clone(),
        copy: plain(output.split(' ').next().unwrap_or("")),
        output,
        output_label: to.label.into(),
        detail,
    })
}

fn currency(a: &Amount, to: &'static str, rates: &Rates) -> Option<Answer> {
    let from = a.money?;
    let input = format!("{} {from}", num(a.v, 10));
    let rate = |c: &str| if c == "EUR" { Some(1.0) } else { rates.eur.get(c).copied().filter(|r| *r > 0.0) };
    let per = rate(to)? / rate(from)?;
    let r = Some(a.v * per).filter(|r| r.is_finite())?;
    let shown = if r.abs() >= 1.0 { fixed(r, 2) } else { num(r, 4) };
    let (x, y, one) = if per >= 1.0 { (from, to, per) } else { (to, from, 1.0 / per) };
    Some(Answer {
        kind: "currency".into(),
        input,
        input_label: label(from).into(),
        output: format!("{shown} {to}"),
        output_label: label(to).into(),
        copy: plain(&shown),
        detail: format!("1 {x} = {} {y} · ECB rate of {}", num(one, 4), day_month(&rates.date)),
    })
}

/// "2026-10-16" → "16 Oct".
fn day_month(date: &str) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let mut p = date.split('-').skip(1).map(|x| x.parse::<usize>().ok());
    match (p.next().flatten(), p.next().flatten()) {
        (Some(m @ 1..=12), Some(d)) => format!("{d} {}", MONTHS[m - 1]),
        _ => date.to_string(),
    }
}
