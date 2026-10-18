# /// script
# requires-python = ">=3.10"
# dependencies = ["python-docx>=1.1", "openpyxl>=3.1", "python-pptx>=1.0", "fpdf2>=2.7", "odfpy>=1.4", "EbookLib>=0.18"]
# ///
"""Writes the demo documents into demo/corpus/home/Documents.

Run with `uv run demo/make_docs.py` (uv builds a throwaway env outside the
repo). Every fact a golden query in tests/golden.json relies on is written
here, so change both together. People and companies are fictional.
"""
import pathlib

from docx import Document
from ebooklib import epub
from fpdf import FPDF
from odf.opendocument import OpenDocumentText
from odf.text import H, P
from openpyxl import Workbook
from pptx import Presentation

HOME = pathlib.Path(__file__).resolve().parent / "corpus" / "home"
DOCS = HOME / "Documents"


def out(rel):
    p = DOCS / rel
    p.parent.mkdir(parents=True, exist_ok=True)
    return p


def blocks(s):
    """Paragraphs of a text block; a leading '# ' marks a heading."""
    return [b.strip() for b in s.strip().split("\n\n") if b.strip()]


def pdf(rel, title, pages):
    """One string per page; pdftotext splits pages on form feeds, so page N is pages[N-1]."""
    doc = FPDF(format="A4")
    doc.set_title(title)
    doc.set_auto_page_break(False)
    for page in pages:
        doc.add_page()
        for b in blocks(page):
            head = b.startswith("# ")
            doc.set_font("Helvetica", "B" if head else "", 15 if head else 11)
            doc.multi_cell(0, 8 if head else 6, b.removeprefix("# "))
            doc.ln(3)
    doc.output(str(out(rel)))


def docx(rel, title, text):
    doc = Document()
    doc.core_properties.title = title
    for b in blocks(text):
        if b.startswith("# "):
            doc.add_heading(b[2:], level=1)
        else:
            doc.add_paragraph(b)
    doc.save(out(rel))


def odt(rel, title, text):
    doc = OpenDocumentText()
    for b in blocks(text):
        doc.text.addElement(H(outlinelevel=1, text=b[2:]) if b.startswith("# ") else P(text=b))
    doc.save(str(out(rel)))


def pptx(rel, slides):
    """slides: (title, body lines, speaker notes); slide N is slides[N-1]."""
    deck = Presentation()
    for title, body, notes in slides:
        s = deck.slides.add_slide(deck.slide_layouts[1])
        s.shapes.title.text = title
        s.placeholders[1].text_frame.text = "\n".join(body)
        if notes:
            s.notes_slide.notes_text_frame.text = notes
    deck.save(out(rel))


def xlsx(rel, sheets):
    wb = Workbook()
    wb.remove(wb.active)
    for name, rows in sheets:
        ws = wb.create_sheet(name)
        for r in rows:
            ws.append(r)
    wb.save(out(rel))


def ebook(rel, title, author, chapters):
    book = epub.EpubBook()
    book.set_identifier("superspace-demo-" + rel)
    book.set_title(title)
    book.set_language("en")
    book.add_author(author)
    items = []
    for i, (head, text) in enumerate(chapters, 1):
        c = epub.EpubHtml(title=head, file_name=f"chap{i}.xhtml", lang="en")
        c.content = f"<h1>{head}</h1>" + "".join(f"<p>{p}</p>" for p in blocks(text))
        book.add_item(c)
        items.append(c)
    book.toc = items
    book.add_item(epub.EpubNcx())
    book.add_item(epub.EpubNav())
    book.spine = ["nav", *items]
    epub.write_epub(str(out(rel)), book)


def text(rel, s):
    out(rel).write_text(s.strip() + "\n", encoding="utf-8")


# ---------------------------------------------------------------- Home

pdf("Home/apartment-lease-new-farm-2026.pdf", "Residential Tenancy Agreement - Unit 14, 88 Merthyr Road, New Farm", [
    """# Residential Tenancy Agreement (General Tenancy, Queensland)

Lessor: Harbourview Property Group Pty Ltd, acting for the owner.

Tenant: Sam Rivera.

Premises: Unit 14, 88 Merthyr Road, New Farm QLD 4005, including one car park (bay 14) and storage cage 6.

Term: fixed term of 12 months, starting 1 August 2026 and ending 31 July 2027.

This agreement is made up of these standard terms, the special terms on page 4 and the entry condition report signed on 1 August 2026.""",
    """# Rent

The rent is $620 per week. Rent is paid fortnightly in advance by direct debit, $1,240 every second Friday, starting Friday 7 August 2026.

Rent may only be increased once in any 12 month period and only with at least two months written notice. Late rent: if rent is unpaid for 7 days the lessor may give a notice to remedy breach.

Water usage is charged to the tenant when the premises are individually metered and water efficient.""",
    """# Rental bond

The tenant must pay a rental bond of $2,400 (two thousand four hundred dollars) before moving in.

The lessor must lodge the bond with the Residential Tenancies Authority (RTA) within 10 days of receiving it. The RTA holds the bond for the whole tenancy.

At the end of the tenancy the bond is refunded once the premises are left in the same condition as at the start, apart from fair wear and tear. A refund is claimed with a Refund of rental bond form signed by both parties.""",
    """# Special terms

Pets: the tenant may keep one small dog under 10 kg with the lessor's written approval. Carpets must be professionally cleaned at the end of the tenancy if a pet was kept. No cats.

Keys and remotes: the tenant receives 2 door keys, 1 swipe tag and 1 garage remote. A replacement garage remote costs $85.

Smoking is not permitted anywhere in the building. Bicycles are stored in the basement cage, not on the balcony.""",
    """# Ending the tenancy

To end the tenancy at the end of the fixed term, the tenant must give at least 2 weeks written notice. Breaking the lease early: the tenant pays rent until a new tenant moves in, plus a reletting fee of one week's rent.

Signed by the tenant, Sam Rivera, on 24 July 2026.

Signed for the lessor, Harbourview Property Group, on 24 July 2026.""",
])

pdf("Home/electricity-bill-sep-2026.pdf", "Sunshine Power - electricity bill September 2026", [
    """# Sunshine Power - Tax invoice

Account 4410 2287 19, Sam Rivera, Unit 14, 88 Merthyr Road, New Farm QLD 4005.

Billing period: 1 July 2026 to 30 September 2026 (92 days).

Amount due: $287.40, due 14 October 2026.

Usage: 1,102 kWh at 26.4 cents per kWh. Daily supply charge 98 cents per day. Solar feed-in credit: none.

Your average daily use was 12.0 kWh, 8 percent lower than the same time last year.""",
])

odt("Home/body-corporate-minutes-sep-2026.odt", "Body corporate minutes", """
# Merthyr Gardens body corporate - committee meeting minutes, 15 September 2026

Present: the chair (Unit 3), the treasurer (Unit 9), the secretary (Unit 14) and the building manager.

Pool: the pool will be closed for resurfacing for three weeks in November 2026, starting Monday 2 November. Quotes from two contractors were compared and Bluewater Pools was chosen at $18,600.

Levies: administrative fund levies increase by 4 percent from 1 January 2027.

Parking: visitor bays are for visitors only; residents parking in them overnight will receive a warning letter.

Next meeting: 17 November 2026 in the ground floor common room.
""")

text("Home/car-service-log.txt", """
Car service log - 2019 Mazda 3 hatch

12 Mar 2025  50,210 km  Logbook service at Valley Auto Care. Oil and filter, brake fluid. $389.
02 Sep 2025  55,040 km  New front tyres (2x Michelin Primacy 4). $412.
12 Sep 2026  60,118 km  Logbook service at Valley Auto Care. Oil, filters, spark plugs, tyres rotated. $545.

Next service due at 70,000 km or by 12 September 2027, whichever comes first.
Wiper blades are getting streaky - replace before summer storms.
""")

text("Home/moving-checklist.md", """
# Moving checklist (New Farm, August 2026)

## Two weeks before
- Book the removalist (Big Blue Removals, Saturday 1 August, 7am, $145 per hour, 2 movers)
- Redirect mail with Australia Post for 6 months
- Tell the electricity company the move date

## Moving week
- Pack the kitchen last; label boxes by room
- Defrost the freezer the night before
- Take photos of the old place for the bond inspection

## After
- Update address on driver licence and Medicare
- Register the bond with the RTA (check the receipt arrives)
- Find the nearest dog park (New Farm Park off-leash area)
""")

# ---------------------------------------------------------------- Meetup

docx("Meetup/meetup-committee-notes-2026-09-29.docx", "Omarchy Brisbane meetup - committee notes", """
# Omarchy Brisbane meetup - committee notes, 29 September 2026

Attendees: Priya Natarajan, Marcus Webb, Lena Hoffmann, Sam Rivera.

# Venue

We looked at three venues. River City Labs costs $350 but only holds 40 people. The State Library meeting room is free but food is not allowed. The Precinct on Brunswick Street holds 80 people and costs $500 including the projector.

Decision: we agreed to hold the October meetup at The Precinct, Level 2, 315 Brunswick Street, Fortitude Valley. Marcus will book it today.

# Date and format

Tuesday 20 October 2026, doors at 6:00 pm, talks from 6:30 pm. Three talks of 15 minutes, then lightning demos.

# Catering

Pizza from Bosco in the Valley, budget $280, plus soft drinks. Lena will check dietary needs in the RSVP form.

# Actions

Marcus: book The Precinct and pay the $500 deposit. Lena: post the event on Meetup.com and Mastodon. Sam: prepare the Super Space demo on the Raspberry Pi. Priya: ask Coral Cloud about sponsoring the pizza.
""")

xlsx("Meetup/meetup-budget-oct-2026.xlsx", [
    ("Budget", [
        ["Item", "Supplier", "Cost (AUD)", "Paid"],
        ["Venue hire", "The Precinct", 500, "deposit"],
        ["Pizza and drinks", "Bosco", 280, "no"],
        ["Stickers", "Sticker Mule", 95, "yes"],
        ["Total", "", 875, ""],
    ]),
    ("Sponsors", [
        ["Sponsor", "Amount (AUD)", "Contact"],
        ["Coral Cloud", 600, "Priya"],
        ["Total", 600, ""],
    ]),
])

text("Meetup/super-space-demo-run-sheet.md", """
# Super Space demo run sheet (Tuesday 20 October, 6:45 pm slot)

1. Open the search with Super+Space on the Raspberry Pi 5.
2. Type "apartment lease bond" and show it opens the PDF on page 3.
3. Type "golden retriever" and show the photos and the moments in the video.
4. Type "the venue costs" and play the voice memo from the right second.
5. Type "songs about a breakup".
6. Answers: "100k idr in usd", "3pm sydney in jakarta".
7. Type "wifi connect" and "awkward" to show settings and emoji.
8. Pull the network cable: it all still works offline.

Backup plan: if the projector fails, use the 27 inch monitor on the side table.
""")

pptx("Meetup/omarchy-meetup-talk.pptx", [
    ("Super Space: Spotlight for Omarchy", ["Omarchy Brisbane meetup, October 2026", "Sam Rivera"], ""),
    ("Why another launcher?", ["Finding a file by what is inside it", "Photos and videos by what they show", "Everything stays on the machine"], ""),
    ("How it works", ["One embedding model for text, images and audio", "Instant name matches, then meaning", "SQLite and a small vector index"], "Mention that the model runs on the Pi's CPU."),
    ("Live demo", ["Raspberry Pi 5, 8 GB, no GPU"], "Follow the demo run sheet."),
    ("Questions", ["github.com/vincenth19/omarchy-super-space", "MIT licensed"], ""),
])

# ---------------------------------------------------------------- Work

pptx("Work/kopi-club-pitch-deck.pptx", [
    ("Kopi Club", ["Fresh Indonesian coffee, delivered to Brisbane offices"], ""),
    ("The problem", ["Office coffee is stale pods and instant", "Good cafes are a 10 minute walk away"], ""),
    ("Our solution", ["Weekly delivery of single-origin beans from Java, Sumatra and Flores", "Roasted in Brisbane on Mondays, delivered on Tuesdays"], ""),
    ("Pricing", ["Starter: $12 per month, 250 g of beans", "Regular: $24 per month, 500 g of beans", "Office: $89 per month, 2 kg of beans plus a free grinder loan", "Annual plans get two months free"], "Most offices pick the Office plan after the first month."),
    ("Market", ["4,200 offices with 10 or more staff in Brisbane", "Australians drink 6 billion cups of coffee a year"], ""),
    ("Traction", ["312 paying subscribers", "18 percent month-on-month growth since March", "Churn under 3 percent"], ""),
    ("Team", ["Ayu Pratama, founder and roaster", "Sam Rivera, engineering", "Marcus Webb, sales"], ""),
    ("The ask", ["Raising $250,000 seed round", "12 months of runway", "Second roaster and a delivery van"], ""),
])

xlsx("Work/bean-there-sales-q3-2026.xlsx", [
    ("Summary", [
        ["Bean There Coffee Co - Q3 2026 sales by store (AUD)", None, None, None],
        ["Store", "Coffees sold", "Revenue", "vs Q2"],
        ["Fortitude Valley", 41210, 298540, "+4%"],
        ["South Bank", 56870, 412380, "+11%"],
        ["Toowong", 33950, 241760, "-2%"],
        ["Chermside", 28400, 203115, "+1%"],
        ["Total", 160430, 1155795, "+4%"],
        ["Best store this quarter: South Bank", None, None, None],
    ]),
    *[(store, [
        ["Month", "Coffees sold", "Coffee revenue", "Food revenue", "Total revenue"],
        *rows,
    ]) for store, rows in [
        ("Fortitude Valley", [["July", 13020, 70300, 23900, 94200], ["August", 13890, 75010, 25480, 100490], ["September", 14300, 77220, 26630, 103850]]),
        ("South Bank", [["July", 18100, 97740, 33100, 130840], ["August", 18650, 100710, 34200, 134910], ["September", 20120, 108650, 37980, 146630]]),
        ("Toowong", [["July", 11400, 61560, 20110, 81670], ["August", 11050, 59670, 19800, 79470], ["September", 11500, 62100, 18520, 80620]]),
        ("Chermside", [["July", 9300, 50220, 16180, 66400], ["August", 9420, 50870, 16400, 67270], ["September", 9680, 52270, 17175, 69445]]),
    ]],
])

docx("Work/project-lighthouse-kickoff-brief.docx", "Project Lighthouse kickoff brief", """
# Project Lighthouse - kickoff brief

Goal: move the billing system from the old PHP monolith to a Rust service by March 2027, without downtime for customers.

Budget: $180,000, covering two contractors for six months and the load-testing environment.

Team: Sam Rivera (tech lead), Priya Natarajan (product), two contract engineers from Fernhill Software.

Milestones: invoices generated by the new service in shadow mode by December 2026; 10 percent of customers by February 2027; all customers by 31 March 2027.

Risks: currency rounding differences between the old and new systems; the tax rules for New Zealand customers.
""")

text("Work/okrs-2026-q4.md", """
# Team OKRs, Q4 2026

## Objective 1: Make billing boring
- KR1: zero billing incidents that reach customers
- KR2: Lighthouse shadow mode matches 99.9% of invoices to the cent

## Objective 2: Faster support
- KR1: first response under 2 hours during business hours
- KR2: cut the support backlog from 140 to 40 tickets

## Objective 3: Grow the team
- KR1: hire one senior engineer in Brisbane
- KR2: every engineer gives one internal tech talk
""")

text("Work/incident-report-2026-09-14.md", """
# Incident report: customer portal outage, 14 September 2026

Duration: 42 minutes (09:12 to 09:54 AEST).

Impact: customers could not log in to the portal or download invoices. About 1,900 failed logins.

Root cause: the TLS certificate for portal.coralcloud.example expired. The renewal job had been failing silently since August because its DNS API token was rotated.

Fix: renewed the certificate by hand at 09:50, then repaired the token.

Follow-ups: alert 21 days before any certificate expires; add the renewal job to the on-call dashboard; document token rotation.
""")

text("Work/team-offsite-stradbroke.html", """
<!doctype html>
<html><head><title>Team offsite - North Stradbroke Island</title>
<style>body{font-family:sans-serif}</style><script>console.log("tracking pixel")</script></head>
<body>
<h1>Team offsite: North Stradbroke Island (Minjerribah)</h1>
<p>Dates: Thursday 5 and Friday 6 November 2026.</p>
<p>Travel: 7:30 am water taxi from Cleveland to Dunwich, then the bus to Point Lookout.</p>
<p>Stay: Point Lookout Beach Resort, rooms are shared two per room.</p>
<h2>Plan</h2>
<ul><li>Thursday: planning for 2027 and the Lighthouse retro</li>
<li>Thursday afternoon: North Gorge walk, look for whales and turtles</li>
<li>Friday: hack morning, ferry home at 3 pm</li></ul>
<p>Bring a hat, reef-safe sunscreen and your laptop charger.</p>
</body></html>
""")

# ---------------------------------------------------------------- Finance

pdf("Finance/Invoices/invoice-INV-1042-riverbend-plumbing.pdf", "Invoice INV-1042 Riverbend Plumbing", [
    """# Riverbend Plumbing - Tax invoice INV-1042

Bill to: Sam Rivera, Unit 14, 88 Merthyr Road, New Farm QLD 4005.

Date: 16 September 2026. Due: 30 September 2026.

Work done: repaired the hot water system, replaced the tempering valve and the pressure relief valve.

Labour 2.5 hours: $250.00. Parts: $135.00.

Total: $385.00 including GST. Pay by bank transfer, reference INV-1042.""",
])

pdf("Finance/Invoices/invoice-2026-08-brightspark-electrical.pdf", "BrightSpark Electrical invoice", [
    """# BrightSpark Electrical - Invoice 2026-08-117

Customer: Sam Rivera.

Date: 21 August 2026.

Installed 4 LED downlights in the living room and replaced the bathroom exhaust fan.

Labour: $380.00. Materials: $260.00.

Total due: $640.00 within 14 days.""",
])

docx("Finance/Invoices/invoice-0193-pixel-and-pine.docx", "Pixel & Pine invoice 0193", """
# Pixel & Pine Web Studio - Invoice 0193

To: Kopi Club Pty Ltd, attention Ayu Pratama.

Date: 2 October 2026.

Website redesign: new home page, subscription checkout and order tracking pages. 38.5 hours at $100 per hour: $3,850.00.

Total: $3,850.00 (no GST, the studio is not registered for GST).

Payment due in 30 days.
""")

odt("Finance/Invoices/invoice-77-greenleaf-gardening.odt", "Greenleaf Gardening invoice 77", """
# Greenleaf Gardening - Invoice 77

Customer: Merthyr Gardens body corporate.

Date: 28 September 2026.

Lawn mowing, edging and hedge trimming for the front garden and courtyard: $120.00.

Green waste removal: included.
""")

text("Finance/tax-2025-26-home-office-notes.md", """
# Tax 2025-26: home office notes

## Working from home
- Method: fixed rate method, 70 cents per hour
- Hours worked from home: 1,240 hours (from my calendar, July 2025 to June 2026)
- Deduction: 1,240 x $0.70 = $868

## Equipment
- Laptop bought 3 March 2026 for $2,199, 90% work use, depreciate over 2 years
- Standing desk $649, depreciate over 10 years

## Records to keep
- Calendar export showing the home office hours
- Receipts for the laptop and desk
- Internet bill: claim 40% work use
""")

xlsx("Finance/household-budget-2026.xlsx", [
    ("Monthly", [
        ["Category", "Budget (AUD)", "September actual"],
        ["Rent", 2687, 2687],
        ["Groceries", 650, 712],
        ["Electricity", 95, 96],
        ["Internet", 85, 85],
        ["Transport", 160, 131],
        ["Eating out", 250, 318],
        ["Dog food and vet", 120, 74],
    ]),
    ("Savings goals", [
        ["Goal", "Target (AUD)", "Saved so far", "By"],
        ["Japan trip (Tokyo and Hokkaido)", 6000, 2350, "April 2027"],
        ["Emergency fund", 10000, 7800, "December 2026"],
        ["New bike", 1400, 900, "February 2027"],
    ]),
])

pdf("Finance/car-insurance-renewal-2026.pdf", "Car insurance renewal", [
    """# Comprehensive car insurance - renewal notice

Policy MZ-3381-2026 for a 2019 Mazda 3 hatch.

Renewal date: 1 November 2026.

Annual premium: $1,142.60, or $99.80 per month.

Basic excess: $800. An extra $400 applies to drivers under 25.

Agreed value: $21,500. Includes windscreen cover with no excess and a hire car after an accident.""",
])

# ---------------------------------------------------------------- Recipes

text("Recipes/banana-bread.md", """
# Banana bread

## Ingredients
- 3 very ripe bananas
- 1/3 cup melted butter
- 3/4 cup brown sugar
- 1 egg, beaten
- 1 teaspoon vanilla
- 1 teaspoon baking soda and a pinch of salt
- 1 1/2 cups plain flour

## Method
1. Heat the oven to 180C (160C fan). Grease a loaf tin.
2. Mash the bananas, stir in the butter, then the sugar, egg and vanilla.
3. Sprinkle over the baking soda and salt, then fold in the flour.
4. Bake for 55 to 60 minutes, until a skewer comes out clean.
5. Cool in the tin for 10 minutes. Best toasted with butter the next day.
""")

text("Recipes/pavlova.txt", """
Christmas pavlova (Nan's recipe)

Serves 8. Make it the day before.

4 egg whites at room temperature, 1 cup caster sugar, 2 teaspoons cornflour, 1 teaspoon white vinegar, 1 teaspoon vanilla.

Beat the egg whites to soft peaks, then add the sugar a spoonful at a time until the meringue is thick and glossy. Fold in the cornflour, vinegar and vanilla. Shape into a 20 cm circle on baking paper.

Bake at 120C for 1 hour 20 minutes, then turn the oven off and leave it inside with the door ajar until cold.

Top with whipped cream, strawberries, kiwi fruit and passionfruit just before serving.
""")

odt("Recipes/beef-rendang.odt", "Beef rendang", """
# Beef rendang (West Sumatran slow-cooked beef curry)

Serves 6. Takes about 3 and a half hours, mostly simmering.

Spice paste: 10 dried chillies soaked, 6 shallots, 4 cloves garlic, 3 cm galangal, 3 cm ginger, 2 stalks lemongrass, 1 teaspoon turmeric. Blend until smooth.

Fry the paste in oil for 5 minutes. Add 1.2 kg chuck steak in large cubes, 400 ml coconut milk, 4 kaffir lime leaves, 1 turmeric leaf if you have it, 1 tablespoon tamarind and 1 tablespoon palm sugar.

Simmer uncovered on low heat for 3 hours, stirring every 15 minutes, until the coconut milk has cooked down and the beef is dark and dry. Stir in 4 tablespoons of toasted grated coconut (kerisik) near the end.

Serve with steamed rice. It tastes even better the next day.
""")

text("Recipes/resep-nasi-goreng.html", """
<!doctype html>
<html lang="id"><head><title>Resep nasi goreng kampung</title></head>
<body>
<h1>Resep nasi goreng kampung</h1>
<p>Untuk 2 porsi. Waktu memasak 15 menit.</p>
<h2>Bahan</h2>
<ul><li>2 piring nasi putih dingin (nasi kemarin lebih baik)</li>
<li>3 siung bawang merah dan 2 siung bawang putih, diiris tipis</li>
<li>2 cabai merah, 1 sendok teh terasi</li>
<li>2 butir telur, 2 sendok makan kecap manis, garam secukupnya</li></ul>
<h2>Cara membuat</h2>
<p>Tumis bawang merah, bawang putih, cabai dan terasi sampai harum. Masukkan telur, orak-arik sebentar. Masukkan nasi, kecap manis dan garam, aduk dengan api besar selama 5 menit.</p>
<p>Sajikan dengan telur mata sapi, kerupuk dan irisan mentimun.</p>
</body></html>
""")

docx("Recipes/lamingtons.docx", "Lamingtons", """
# Lamingtons

Makes 16 squares.

Sponge: bake a plain butter sponge in a 20 x 30 cm tin at 180C for 25 minutes. Cool completely, then freeze for 30 minutes so it does not crumble. Cut into 16 squares.

Icing: 4 cups icing sugar, 1/3 cup cocoa, 15 g butter and 1/2 cup milk, stirred over a bowl of hot water until smooth.

Dip each square in the chocolate icing, then roll in desiccated coconut. Leave on a rack to set for an hour.

For the meetup: make a double batch, they go fast.
""")

# ---------------------------------------------------------------- Travel

pdf("Travel/tasmania-camping-itinerary-dec-2026.pdf", "Tasmania camping itinerary December 2026", [
    """# Tasmania camping trip - 18 to 24 December 2026

Seven days in a campervan, starting and ending in Hobart.

Campervan: Apollo Euro Camper, pick up Hobart depot Friday 18 December at 10 am. Booking reference APL-58213.

Travellers: Sam and Lena.

Rough route: Hobart, Mount Field, Freycinet, Bay of Fires, Cradle Mountain, Strahan, back to Hobart.""",
    """# Days 1 to 3

Day 1 (Fri 18 Dec): Hobart to Mount Field National Park. Walk to Russell Falls and Horseshoe Falls. Camp at the Mount Field campground.

Day 2 (Sat 19 Dec): Drive to Freycinet National Park. Camp at Richardsons Beach campground near Coles Bay (powered site 23, booked through the parks ballot).

Day 3 (Sun 20 Dec): Early walk up to the Wineglass Bay lookout, then down to the beach for a swim. Oysters at Freycinet Marine Farm for lunch.""",
    """# Days 4 to 7

Day 4 (Mon 21 Dec): North to the Bay of Fires. Free camping at Cosy Corner, orange lichen rocks at sunset.

Day 5 (Tue 22 Dec): Long drive to Cradle Mountain. Walk the Dove Lake circuit (6 km, about 2 hours). Stay at the Discovery Parks campground.

Day 6 (Wed 23 Dec): Down the west coast to Strahan, Gordon River cruise at 8:30 am.

Day 7 (Thu 24 Dec): Back to Hobart, return the van by 3 pm, Christmas Eve dinner at the waterfront.""",
    """# Packing list

Thermal tops and a warm beanie, even in December. Rain jacket. Hiking boots. Head torch. Swimmers and a quick-dry towel.

Holiday pass for the national parks: $90 per vehicle, buy online before we fly.

Food for the first two nights; there is a supermarket in Swansea. Gas canister cannot go on the plane: buy one in Hobart.""",
])

text("Travel/flight-booking-bne-hba.txt", """
Booking confirmation - Virgin Australia
Booking reference: QX7P2L

Passengers: Sam Rivera, Lena Hoffmann

Flight VA1532, Brisbane (BNE) to Hobart (HBA)
Friday 18 December 2026, departs 6:15 am, arrives 9:05 am
Baggage: 23 kg each

Return flight VA1537, Hobart (HBA) to Brisbane (BNE)
Thursday 24 December 2026, departs 6:40 pm, arrives 9:20 pm
""")

# ---------------------------------------------------------------- Career

pdf("Career/sam-rivera-cv-2026.pdf", "Sam Rivera - CV", [
    """# Sam Rivera - Senior software engineer

Brisbane, Queensland. sam.rivera@example.com

# Experience

Coral Cloud, Brisbane - Senior software engineer, 2022 to now. Lead engineer on the billing platform. Moved invoice generation to Rust, cutting run time from 40 minutes to 3. Mentor to four junior engineers.

Fernhill Software, Gold Coast - Software engineer, 2018 to 2022. Built booking systems for tour operators in Python and PostgreSQL.""",
    """# Education

Bachelor of Information Technology, Queensland University of Technology (QUT), 2015 to 2018. Major in computer science, graduated with distinction.

# Skills

Rust, Python, TypeScript, PostgreSQL, Linux, Kubernetes, Raspberry Pi tinkering.

# Interests

Coffee roasting, trail running, organising the Omarchy Brisbane meetup.

# Referees

Available on request.""",
])

docx("Career/cover-letter-coral-cloud-staff-engineer.docx", "Cover letter", """
# Cover letter - Staff engineer, Coral Cloud

Dear hiring panel,

I would like to apply for the staff engineer role in the payments group. Over the last four years I have led the billing platform, and Project Lighthouse is my proposal for moving it to Rust without downtime.

I enjoy work that makes systems simpler and quieter, and I spend my weekends running the Omarchy meetup and teaching people Linux.

Kind regards,
Sam Rivera
""")

# ---------------------------------------------------------------- Personal

docx("Personal/best-man-speech-tom-and-jess.docx", "Best man speech for Tom and Jess", """
# Best man speech - Tom and Jess, 7 November 2026

Good evening everyone. For those who don't know me, I'm Sam, and I've known Tom since we were eleven.

Tom and Jess met at a trivia night in West End. Their team was called Quiz Khalifa. They lost, but Tom got her number, so he says it was a win.

I have to tell you about the camping trip to Noosa. Tom packed the car, he packed the esky, he packed three kinds of hot sauce. He did not pack the tent. We slept in the car, and Jess still agreed to marry him. That is love.

Jess, thank you for making my best mate so happy. Please raise your glasses: to Tom and Jess.
""")

text("Personal/gift-ideas.txt", """
Gift ideas

Mum (birthday 3 Nov): pottery class voucher at Clayworks West End, about $95. Or a new gardening hat.
Dad (Christmas): a good chef's knife, he keeps borrowing mine.
Lena (birthday Jan): headlamp for the Tasmania trip? No, she has one. Concert tickets.
Tom and Jess wedding: contribute to the honeymoon fund (Japan).
""")

text("Personal/book-club-reading-list.md", """
# Book club reading list, 2026

- January: Pride and Prejudice by Jane Austen
- March: The Time Machine by H. G. Wells
- May: Frankenstein by Mary Shelley
- July: Alice's Adventures in Wonderland by Lewis Carroll (re-read, it was a vote)
- September: The Call of the Wild by Jack London
- November: Little Women by Louisa May Alcott

We meet on the last Sunday of the month at Lena's place. Bring snacks.
""")

text("Personal/journal-2026-09.md", """
# Journal, September 2026

## 6 September
First surf lesson at Currumbin. Stood up twice. Very sore shoulders.

## 13 September
Long run along the river, 18 km. The half marathon plan is working.

## 20 September
Tom asked me to be his best man. Started thinking about the speech. The Noosa tent story has to go in.

## 27 September
Spent the whole day on the Super Space demo. The photo search found the beach photos from Bali by meaning alone.
""")

text("Personal/surat-untuk-nenek.txt", """
Surat untuk Nenek

Nenek yang tersayang,

Apa kabar? Semoga Nenek sehat selalu di Denpasar. Di Brisbane sekarang sudah mulai musim panas dan cuacanya sangat cerah.

Aku ingin memberi kabar bahwa aku akan pulang ke Bali bulan Januari, tanggal 8 sampai 22 Januari 2027. Aku akan membawa kopi dari Brisbane untuk Nenek, dan aku ingin belajar memasak rendang lagi bersama Nenek.

Peluk dan cium dari jauh,
Sam
""")

xlsx("Personal/half-marathon-training-plan.xlsx", [
    ("Plan", [
        ["Week", "Tuesday", "Thursday", "Sunday long run", "Total km"],
        [1, "5 km easy", "6 km tempo", "12 km", 23],
        [2, "6 km easy", "6 km intervals", "14 km", 26],
        [3, "6 km easy", "8 km tempo", "16 km", 30],
        [4, "5 km easy", "5 km easy", "10 km (rest week)", 20],
        [5, "6 km easy", "8 km intervals", "18 km", 32],
        [6, "8 km easy", "8 km tempo", "19 km", 35],
        [7, "6 km easy", "6 km easy", "12 km taper", 24],
        [8, "4 km easy", "Rest", "Race: Brisbane half marathon, 21.1 km, Sunday 15 November", 25],
    ]),
])

pdf("Health/physio-knee-exercises.pdf", "Knee rehab exercises", [
    """# Knee rehabilitation exercises - right knee (patellofemoral pain)

Do these once a day for 6 weeks. Stop if the pain goes above 4 out of 10.

1. Straight leg raises: 3 sets of 12, hold for 3 seconds at the top.

2. Wall sits: 3 times 30 seconds, knees bent to 45 degrees.

3. Step-downs from a 15 cm step: 3 sets of 10, slow and controlled.

4. Clamshells with a band: 3 sets of 15 each side.

Review appointment: Thursday 29 October 2026, 4:15 pm, with the physio at Riverside Physio.""",
])

# ---------------------------------------------------------------- Notes

text("Notes/wireguard-vpn-setup-raspberry-pi.md", """
# WireGuard VPN on the Raspberry Pi

How I set up the home VPN so the laptop can reach the Pi from anywhere.

## Install
    sudo pacman -S wireguard-tools

## Keys
    wg genkey | tee server.key | wg pubkey > server.pub
    wg genkey | tee laptop.key | wg pubkey > laptop.pub

## /etc/wireguard/wg0.conf on the Pi
    [Interface]
    Address = 10.8.0.1/24
    ListenPort = 51820
    PrivateKey = <server.key>

    [Peer]
    PublicKey = <laptop.pub>
    AllowedIPs = 10.8.0.2/32
    PersistentKeepalive = 25

## Start on boot
    sudo systemctl enable --now wg-quick@wg0

Forward UDP port 51820 on the router to the Pi. Check with `sudo wg show`.
""")

text("Notes/linux-commands-cheatsheet.txt", """
Linux commands I keep forgetting

Find big files:            du -ah ~ | sort -h | tail -20
What is using a port:      ss -ltnp | grep 8080
Follow a service log:      journalctl --user -fu superspace
Disk usage by folder:      ncdu ~
Extract a tar.gz:          tar xzf file.tar.gz
Copy with progress:        rsync -ah --progress src/ dest/
Which package owns a file: pacman -Qo /usr/bin/ls
""")

text("Study/rust-ownership-notes.md", """
# Rust ownership notes

## The three rules
1. Each value has one owner.
2. There can only be one owner at a time.
3. When the owner goes out of scope, the value is dropped.

## Borrowing
- Any number of shared references (&T), or exactly one mutable reference (&mut T), never both at once.
- References must always be valid: the borrow checker rejects references that outlive their data.

## Tips
- Clone when fighting the borrow checker in non-hot code; optimise later.
- Prefer passing &str over &String, and &[T] over &Vec<T>.
""")

ebook("Books/coffee-brewing-field-guide.epub", "A Field Guide to Brewing Coffee", "Ayu Pratama", [
    ("Pour over", """A pour over makes a clean, bright cup. Use a ratio of 1 to 16: 15 g of coffee for 250 g of water.

Water should be just off the boil, around 94 degrees Celsius. Bloom with twice the coffee's weight in water for 30 seconds, then pour slowly in circles. The whole brew should take about 3 minutes."""),
    ("French press", """Use a coarse grind and a ratio of 1 to 15. Steep for 4 minutes, break the crust, skim the foam, then press gently.

Pour it all out straight away, or the coffee keeps brewing and turns bitter."""),
    ("AeroPress", """The AeroPress forgives mistakes. Try 15 g of fine coffee, 200 g of water at 85 degrees, stir for 10 seconds, steep for 1 minute and press for 30 seconds."""),
    ("Cold brew", """Cold brew is steeped, not brewed. Mix 100 g of coarse coffee with 1 litre of cold water.

Leave it to steep in the fridge for 18 hours, then strain through a paper filter. It keeps for a week. Dilute one to one with milk or water."""),
])

print(f"documents written under {DOCS}")
