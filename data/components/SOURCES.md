# Where the component tables were checked (2026-11-02)

The values were typed for this library and then compared, row by row, with the public pages below; they were
not copied from them in bulk, and nothing comes from commercial CAD. A row of `families.csv` names what its table
was compared with; an empty `checked` means **not compared yet**. Nothing here replaces the standard itself: check
a value against the standard or the manufacturer before relying on it in production.

| Source | Used for |
|---|---|
| [fasten.it norms](https://www.fasten.it/en/Norms) (ISO 4017, 4762, 4032, 4035, 7089, 10642; DIN 933, 934, 985, 471, 472, 127, 6885-1; ASME B18.2.1 Table 6) | fasteners, rings, keys |
| [wermac.org metric bolts](https://www.wermac.org/bolts/metricBDC.html) | head heights of DIN 931 / 933, ISO 4014 / 4017 |
| [Westfield Fasteners specifications](https://westfieldfasteners.co.uk/Standards/Nut-Hex-M.pdf) (DIN 933, DIN 934) | across flats, heights |
| [Wurth eShop](https://eshop.wuerth.de/) product pages | single nut heights (ISO 4032 M16, M24; DIN 934 M6) |
| [GMN ORS catalogue](https://www.gmnbt.com/gmn-ors-catalog-n2.pdf) | deep groove bearings 60, 62, 63 |
| NSK, NTN, SKF product listings at [RS](https://uk.rs-online.com/) and [Coolen](https://www.coolenbearings.com/) | samples of tapered, cylindrical and thrust bearings |

Findings of the comparison:

- Corrected: DIN 933 and DIN 931 head heights (they are the ISO ones; only some across flats differ), ISO 10642 head heights.
- ISO 4017 M30 / M36: 18.7 / 22.5 is the middle of the standard's limits; one page rounds them to 19 / 23.
- ISO 4032 M6 stays 5.2 although fasten.it prints 5; that page also has M24 at 19, below M22, while Wurth gives 21.5.
  **Unresolved until checked against the standard.**

Bearing boundary dimensions follow ISO 15, so makers agree on them; samples of SKF, NSK and NTN were compared.

**Not used:** MISUMI. Its terms for CAD data forbid obtaining it in bulk and reusing it beyond designing with its
products; its catalogue is not a source for this library unless MISUMI agrees.
