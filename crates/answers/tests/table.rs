//! Every answer the launcher must give (and must not give), at a fixed moment:
//! Sun 18 Oct 2026 04:00 UTC = 14:00 in Brisbane, with fixed rates.
use ss_answers::{answer, Rates};

const NOW: i64 = 1792296000;
const HOME: &str = "Australia/Brisbane";

fn rates() -> Rates {
    let eur = [
        ("USD", 1.2), ("AUD", 1.8), ("GBP", 0.9), ("IDR", 19800.0), ("JPY", 180.0), ("SGD", 1.5), ("MYR", 5.4),
        ("THB", 39.6), ("INR", 99.0), ("KRW", 1650.0), ("NZD", 2.0), ("CNY", 8.4),
    ];
    Rates { date: "2026-10-16".into(), eur: eur.iter().map(|(c, r)| (c.to_string(), *r)).collect() }
}

/// (query, kind, output); kind "" means no answer.
#[rustfmt::skip]
const CASES: &[(&str, &str, &str)] = &[
    // Failed in the old launcher.
    ("5'11\" in cm", "units", "180.34 cm"),
    ("1 tbsp in ml", "units", "14.7868 mL"),
    ("3 cups to grams", "units", "709.765 g"),
    ("60 mph in km/h", "units", "96.5606 km/h"),
    ("100 km/h to mph", "units", "62.1371 mph"),
    ("1 gb in mb", "units", "1,000 MB"),
    ("500 mb to gb", "units", "0.5 GB"),
    ("2 hours in minutes", "units", "120 min"),
    ("1 acre to m2", "units", "4,046.86 m²"),
    ("30 sq m in sq ft", "units", "322.917 ft²"),
    ("1 ton in kg", "units", "1,000 kg"),
    ("10 stone in kg", "units", "63.5029 kg"),
    ("half a mile in metres", "units", "804.672 m"),
    ("1e3 m in km", "units", "1 km"),
    ("1m idr to aud", "currency", "90.91 AUD"),
    ("100k idr in usd", "currency", "6.06 USD"),
    ("20 bucks in aud", "currency", "30.00 AUD"),
    ("usd to idr", "currency", "16,500.00 IDR"),
    ("15% of 80", "percent", "12"),
    ("12pm here in london", "time", "3:00 AM"),
    ("3pm sydney in jakarta", "time", "11:00 AM"),
    ("time in berlin", "time", "06:00"),
    // Worked before; must keep working.
    ("5 km", "units", "3.10686 mi"),
    ("72 f", "units", "22.2222 °C"),
    ("100 usd to idr", "currency", "1,650,000.00 IDR"),
    ("€20 to £", "currency", "18.00 GBP"),
    ("2^10", "math", "1,024"),
    ("sqrt(144)", "math", "12"),
    ("(3+4)*5", "math", "35"),
    // Length and height.
    ("5 ft 11 in to cm", "units", "180.34 cm"),
    ("5 feet 11 inches in cm", "units", "180.34 cm"),
    ("5ft 11 in cm", "units", "180.34 cm"),
    ("5’11” in cm", "units", "180.34 cm"),
    ("5'11\"", "units", "180.34 cm"),
    ("180 cm in feet", "units", "5.90551 ft"),
    ("6 ft", "units", "1.8288 m"),
    ("1 m in ft", "units", "3.28084 ft"),
    ("1 mile in km", "units", "1.60934 km"),
    ("26.2 miles to km", "units", "42.1648 km"),
    ("100 km in miles", "units", "62.1371 mi"),
    ("5 kilometres in miles", "units", "3.10686 mi"),
    ("1 inch in mm", "units", "25.4 mm"),
    ("1 mm in inches", "units", "0.0393701 in"),
    ("1 yard in metres", "units", "0.9144 m"),
    ("1 nautical mile in km", "units", "1.852 km"),
    ("1,000 m to km", "units", "1 km"),
    ("km to miles", "units", "0.621371 mi"),
    ("inch to cm", "units", "2.54 cm"),
    ("a mile in km", "units", "1.60934 km"),
    ("a dozen inches in cm", "units", "30.48 cm"),
    ("how many cm in an inch", "units", "2.54 cm"),
    ("12 in to cm", "units", "30.48 cm"),
    ("1 in in cm", "units", "2.54 cm"),
    ("5 km 300 m in miles", "units", "3.29327 mi"),
    // Mass.
    ("10 kg to lb", "units", "22.0462 lb"),
    ("150 lbs in kg", "units", "68.0389 kg"),
    ("2.5 kg in pounds", "units", "5.51156 lb"),
    ("500 g to lb", "units", "1.10231 lb"),
    ("1 oz in grams", "units", "28.3495 g"),
    ("1 kg to g", "units", "1,000 g"),
    ("5 g", "units", "0.17637 oz"),
    ("1 stone to lb", "units", "14 lb"),
    ("ten stone in kg", "units", "63.5029 kg"),
    ("1 us ton in kg", "units", "907.185 kg"),
    ("10 pounds", "units", "4.53592 kg"),
    // Volume and cooking.
    ("1 cup in ml", "units", "236.588 mL"),
    ("2 tsp to ml", "units", "9.85784 mL"),
    ("1 l to cups", "units", "4.22675 cups"),
    ("250 ml in cups", "units", "1.05669 cups"),
    ("1 metric cup in ml", "units", "250 mL"),
    ("1 gallon in litres", "units", "3.78541 L"),
    ("1 uk gallon in litres", "units", "4.54609 L"),
    ("100 ml to fl oz", "units", "3.3814 fl oz"),
    ("1 pint in ml", "units", "473.176 mL"),
    ("1 ml to tsp", "units", "0.202884 tsp"),
    ("how many ml in a tbsp", "units", "14.7868 mL"),
    ("500 g to cups", "units", "2.11338 cups"),
    ("1 l of water in kg", "units", "1 kg"),
    ("1/2 cup in ml", "units", "118.294 mL"),
    ("quarter mile in feet", "units", "1,320 ft"),
    ("180 cm to feet and inches", "units", "5.90551 ft"),
    // Temperature.
    ("100 c to f", "units", "212 °F"),
    ("-40 c in f", "units", "-40 °F"),
    ("0 k to c", "units", "-273.15 °C"),
    ("72 °f to °c", "units", "22.2222 °C"),
    ("72°F", "units", "22.2222 °C"),
    ("100°c", "units", "212 °F"),
    ("30 celsius in fahrenheit", "units", "86 °F"),
    // Speed and area.
    ("60 mph", "units", "96.5606 km/h"),
    ("100 kmh to mph", "units", "62.1371 mph"),
    ("10 m/s in km/h", "units", "36 km/h"),
    ("1 kn to km/h", "units", "1.852 km/h"),
    ("30 sq m", "units", "322.917 ft²"),
    ("1 hectare in acres", "units", "2.47105 ac"),
    ("1 km2 to sq mi", "units", "0.386102 mi²"),
    ("100 sq ft to m2", "units", "9.2903 m²"),
    ("100 square feet in square metres", "units", "9.2903 m²"),
    ("2 m² in sq ft", "units", "21.5278 ft²"),
    // Data and durations.
    ("1 tb to gb", "units", "1,000 GB"),
    ("1 gib to mib", "units", "1,024 MiB"),
    ("8 bits to bytes", "units", "1 B"),
    ("1 gigabyte in megabytes", "units", "1,000 MB"),
    ("90 minutes in hours", "units", "1.5 h"),
    ("1 day in hours", "units", "24 h"),
    ("1 week in days", "units", "7 days"),
    ("3 days to hours", "units", "72 h"),
    ("1 year in days", "units", "365.243 days"),
    ("1 fortnight in days", "units", "14 days"),
    ("1 h 30 min in minutes", "units", "90 min"),
    ("two hours in minutes", "units", "120 min"),
    ("an hour in seconds", "units", "3,600 s"),
    // Currency.
    ("100 usd", "currency", "150.00 AUD"),
    ("50 aud", "currency", "33.33 USD"),
    ("$20", "currency", "30.00 AUD"),
    ("a$50 to usd", "currency", "33.33 USD"),
    ("us$100 to aud", "currency", "150.00 AUD"),
    ("s$100 in aud", "currency", "120.00 AUD"),
    ("rp 100.000 to aud", "currency", "9.09 AUD"),
    ("rp 100k", "currency", "9.09 AUD"),
    ("Rp100rb in aud", "currency", "9.09 AUD"),
    ("1.5bn idr in usd", "currency", "90,909.09 USD"),
    ("1 million yen to aud", "currency", "10,000.00 AUD"),
    ("5000 yen", "currency", "50.00 AUD"),
    ("100 quid in aud", "currency", "200.00 AUD"),
    ("10 pounds to aud", "currency", "20.00 AUD"),
    ("100 ringgit to idr", "currency", "366,666.67 IDR"),
    ("1000 baht to aud", "currency", "45.45 AUD"),
    ("50 sgd to myr", "currency", "180.00 MYR"),
    ("100 euros to usd", "currency", "120.00 USD"),
    ("20 eur in gbp", "currency", "18.00 GBP"),
    ("1 aud to idr", "currency", "11,000.00 IDR"),
    ("how many yen in a dollar", "currency", "150.00 JPY"),
    ("1 idr to aud", "currency", "0.00009091 AUD"),
    ("100 inr to aud", "currency", "1.82 AUD"),
    ("1000 krw to usd", "currency", "0.7273 USD"),
    ("2 million rupiah to aud", "currency", "181.82 AUD"),
    ("50 us dollars in aud", "currency", "75.00 AUD"),
    ("20 australian dollars to usd", "currency", "13.33 USD"),
    ("100 nzd to aud", "currency", "90.00 AUD"),
    ("1000 thai baht in aud", "currency", "45.45 AUD"),
    ("what is 100 usd in aud?", "currency", "150.00 AUD"),
    ("100 usd to xyz", "", ""),
    ("100 chf to aud", "", ""), // CHF missing from these test rates
    // Maths and percentages.
    ("2 + 2", "math", "4"),
    ("10 / 4", "math", "2.5"),
    ("1/3", "math", "0.3333333333"),
    ("2 * (3 + 4)", "math", "14"),
    ("-5 + 3", "math", "-2"),
    ("2 ** 8", "math", "256"),
    ("5!", "math", "120"),
    ("50 % 3", "math", "2"),
    ("17 mod 5", "math", "2"),
    ("3 x 4", "math", "12"),
    ("3 × 4", "math", "12"),
    ("1920x1080", "math", "2,073,600"),
    ("sqrt 16 + 9", "math", "13"),
    ("√16", "math", "4"),
    ("2pi", "math", "6.283185307"),
    ("sin(pi/2)", "math", "1"),
    ("sin 30°", "math", "0.5"),
    ("log(1000)", "math", "3"),
    ("ln(e)", "math", "1"),
    ("log2(1024)", "math", "10"),
    ("abs(-7)", "math", "7"),
    ("cbrt(27)", "math", "3"),
    ("10 plus 5", "math", "15"),
    ("6 times 7", "math", "42"),
    ("100 divided by 8", "math", "12.5"),
    ("3 squared", "math", "9"),
    ("2 to the power of 8", "math", "256"),
    ("0.1 + 0.2", "math", "0.3"),
    ("1,000 * 3", "math", "3,000"),
    ("1e3 + 1", "math", "1,001"),
    ("what is 7 * 6", "math", "42"),
    ("12 * 12 =", "math", "144"),
    ("1k + 500", "math", "1,500"),
    ("2^0.5", "math", "1.414213562"),
    ("(1+2)(3+4)", "math", "21"),
    ("2^64", "math", "1.844674407e19"),
    ("80 + 15%", "percent", "92"),
    ("100 - 15%", "percent", "85"),
    ("what is 20% of 50", "percent", "10"),
    ("20% off 80", "percent", "64"),
    ("12.5% of 200", "percent", "25"),
    ("15 percent of 80", "percent", "12"),
    ("10% + 5%", "percent", "15%"),
    ("1/0", "", ""),
    ("sqrt(-1)", "", ""),
    // Time zones.
    ("now in tokyo", "time", "13:00"),
    ("9am PST to AEST", "time", "3:00 AM"),
    ("london time", "time", "05:00"),
    ("what time is it in new york", "time", "00:00"),
    ("time in bali", "time", "12:00"),
    ("time in india", "time", "09:30"),
    ("time in kathmandu", "time", "09:45"),
    ("time in california", "time", "21:00"),
    ("3pm london", "time", "12:00 AM"),
    ("15:00 in tokyo", "time", "14:00"),
    ("noon in new york", "time", "10:00 PM"),
    ("9:30am perth to sydney", "time", "12:30 PM"),
    ("10am utc to brisbane", "time", "8:00 PM"),
    ("8pm here in utc+8", "time", "6:00 PM"),
    ("5 pm melbourne in bandung", "time", "1:00 PM"),
    ("3pm london time", "time", "12:00 AM"),
    ("time in xyzzy", "", ""),
    // Ordinary searches: never answers.
    ("golden retriever", "", ""), ("lease", "", ""), ("1password", "", ""), ("2048 game", "", ""),
    ("windows 11", "", ""), ("iphone 15", "", ""), ("2048", "", ""), ("4k", "", ""), ("5g", "", ""),
    ("1st", "", ""), ("21st century", "", ""), ("covid 19", "", ""), ("ps5", "", ""), ("rtx 4090", "", ""),
    ("m2 macbook", "", ""), ("1080p", "", ""), ("60fps", "", ""), ("3 musketeers", "", ""),
    ("7 days to die", "", ""), ("day to day", "", ""), ("time machine", "", ""), ("screen time", "", ""),
    ("2026-10-18", "", ""), ("1-800-555-1234", "", ""), ("555-1234", "", ""), ("2026-10", "", ""), ("10-5", "math", "5"), ("18/10/2026", "", ""), ("192.168.1.1", "", ""),
    ("v1.2.3", "", ""), ("10:30", "", ""), ("16:9", "", ""), ("mp3 to wav", "", ""), ("pdf to word", "", ""),
    ("usd", "", ""), ("km", "", ""), ("pi", "", ""), ("e", "", ""), ("12pm", "", ""), ("london", "", ""),
    ("berlin wall", "", ""), ("new york times", "", ""), ("office 365", "", ""), ("top 10", "", ""),
    ("1 to 10", "", ""), ("5 in 1", "", ""), ("3 in a row", "", ""), ("catch 22", "", ""), ("route 66", "", ""),
    ("area 51", "", ""), ("blink 182", "", ""), ("100 days of code", "", ""), ("half life 2", "", ""),
    ("one direction", "", ""), ("a to z", "", ""), ("15%", "", ""), ("-5", "", ""), ("8 gb", "", ""),
    ("30 sec", "", ""), ("1 day", "", ""), ("100m", "", ""), ("3m", "", ""), ("hello world", "", ""),
    ("", "", ""), ("?", "", ""), ("rm -rf", "", ""), ("c++", "", ""), ("how to", "", ""), ("1 in 4", "", ""),
    ("50 cent", "", ""), ("7 eleven", "", ""), ("100", "", ""), ("a dozen", "", ""), ("half", "", ""),
    ("time to eat", "", ""), ("the time traveler", "", ""), ("24 hours in a day", "", ""),
    ("tokyo drift", "", ""), ("sydney opera house", "", ""), ("chrome", "", ""), ("dark mode", "", ""),
    ("wifi", "", ""), ("bluetooth settings", "", ""), ("2 factor", "", ""), ("4 seasons", "", ""),
    ("c to c", "", ""), ("a cup", "", ""), ("one mile", "", ""), ("12 angry men", "", ""),
];

#[test]
fn table() {
    let r = rates();
    let mut bad = Vec::new();
    for (q, kind, out) in CASES {
        let got = answer(q, Some(&r), NOW, HOME);
        let ok = match &got {
            None => kind.is_empty(),
            Some(a) => a.kind == *kind && a.output == *out,
        };
        if !ok {
            bad.push(format!("{q:?}: want {kind} {out:?}, got {:?}", got.map(|a| (a.kind, a.output))));
        }
    }
    assert!(CASES.len() >= 150, "{} cases", CASES.len());
    assert!(bad.is_empty(), "{} of {} wrong:\n{}", bad.len(), CASES.len(), bad.join("\n"));
}

fn card(q: &str) -> [String; 6] {
    let a = answer(q, Some(&rates()), NOW, HOME).unwrap_or_else(|| panic!("no answer for {q:?}"));
    [a.input, a.input_label, a.output, a.output_label, a.copy, a.detail]
}

#[test]
fn cards() {
    let s = |v: [&str; 6]| v.map(String::from);
    assert_eq!(card("12pm here in london"), s(["12:00 PM", "Brisbane, AEST", "3:00 AM", "London, BST", "3:00 AM", "Sun 18 Oct · London is 9 h behind Brisbane"]));
    assert_eq!(card("3pm sydney in jakarta"), s(["3:00 PM", "Sydney, AEDT", "11:00 AM", "Jakarta, WIB", "11:00 AM", "Sun 18 Oct · Jakarta is 4 h behind Sydney"]));
    assert_eq!(card("time in berlin"), s(["14:00", "Brisbane, AEST", "06:00", "Berlin, CEST", "06:00", "Sun 18 Oct · Berlin is 8 h behind Brisbane"]));
    assert_eq!(card("9am pst to aest"), s(["9:00 AM", "PST, UTC-8", "3:00 AM", "AEST, UTC+10", "3:00 AM", "Sun 18 Oct · AEST is 18 h ahead of PST"]));
    assert_eq!(card("time in kathmandu")[3], "Kathmandu, UTC+5:45");
    assert_eq!(card("3pm london")[5], "Mon 19 Oct · Brisbane is 9 h ahead of London");
    assert_eq!(card("time in brisbane")[5], "Sun 18 Oct · Same time as Brisbane");
    assert_eq!(card("1m idr to aud"), s(["1,000,000 IDR", "Indonesian rupiah", "90.91 AUD", "Australian dollars", "90.91", "1 AUD = 11,000 IDR · ECB rate of 16 Oct"]));
    assert_eq!(card("usd to idr")[5], "1 USD = 16,500 IDR · ECB rate of 16 Oct");
    assert_eq!(card("5'11\" in cm"), s(["5 ft 11 in", "Feet and inches", "180.34 cm", "Centimetres", "180.34", ""]));
    assert_eq!(card("1 tbsp in ml")[5], "US tablespoon; Australian is 20 mL, UK 15 mL");
    assert_eq!(card("3 cups to grams")[5], "Assuming water (1 mL weighs 1 g); other ingredients differ");
    assert_eq!(card("1 ton in kg")[5], "Metric tonne; a US ton is 907.18 kg");
    assert_eq!(card("1 gb in mb")[5], "Binary units: 1 GiB = 1,024 MiB");
    assert_eq!(card("180 cm in feet")[5], "5 ft 10.9 in");
    assert_eq!(card("half a mile in metres")[..2], s(["0.5 mi", "Miles", "", "", "", ""])[..2]);
    assert_eq!(card("1,000 * 3")[4], "3000");
}

#[test]
fn no_rates_no_money() {
    assert_eq!(answer("100 usd to idr", None, NOW, HOME), None);
    assert!(answer("5 km", None, NOW, HOME).is_some());
}

#[test]
fn home_currency_follows_the_time_zone() {
    let r = rates();
    let out = |q: &str, tz: &str| answer(q, Some(&r), NOW, tz).map(|a| a.output);
    assert_eq!(out("100 usd", "Asia/Jakarta").as_deref(), Some("1,650,000.00 IDR"));
    assert_eq!(out("100 usd", "America/New_York").as_deref(), Some("83.33 EUR"));
    assert_eq!(out("100 usd", "Nowhere/Invalid").as_deref(), Some("83.33 EUR"));
    assert_eq!(out("time in berlin", "Nowhere/Invalid").as_deref(), Some("06:00"));
    let a = answer("time in berlin", Some(&r), NOW, "Nowhere/Invalid").unwrap();
    assert_eq!((a.input.as_str(), a.input_label.as_str()), ("04:00", "UTC"));
}
