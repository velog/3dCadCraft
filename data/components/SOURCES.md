# Where the component tables were checked (2026-11-02, 2026-11-03)

The values were typed for this library and then compared, row by row, with the public pages below; they were
not copied from them in bulk, and nothing comes from commercial CAD. A row of `families.csv` names what its table
was compared with; an empty `checked` means **not compared yet**. Nothing here replaces the standard itself: check
a value against the standard or the manufacturer before relying on it in production.

**When sources disagree, both values stay** (owner's rule, 2026-11-03): makers and editions really differ. The
second value is a row of its own, named after the difference: `M10 (s 16)`, `R6 (B 5.558)`. A value only we had
typed, which a source contradicts, is simply corrected.

| Source | Used for |
|---|---|
| [fasten.it norms](https://www.fasten.it/en/norms) (`/en/norms/norm/<norm>`: DIN, ISO, EN, ASME pages) | fasteners, pins, rings, keys, inch screws and nuts |
| [wermac.org metric bolts](https://www.wermac.org/bolts/metricBDC.html) | head heights of DIN 931 / 933, ISO 4014 / 4017 |
| [Westfield Fasteners specifications](https://westfieldfasteners.co.uk/Standards/Nut-Hex-M.pdf) (DIN 933, DIN 934) | across flats, heights |
| [Wurth eShop](https://eshop.wuerth.de/) product pages | single nut heights (ISO 4032 M16, M24; DIN 934 M6) |
| [GMN ORS catalogue](https://www.gmnbt.com/gmn-ors-catalog-n2.pdf) | deep groove bearings 60, 62, 63 |
| [Koyo product pages](https://koyo.jtekt.co.jp/en/products/) | every row of the series 68, 72, 12, 222 and the small bores |
| NSK, NTN, SKF, INA product listings at [RS](https://uk.rs-online.com/) and [Coolen](https://www.coolenbearings.com/) | samples of tapered, cylindrical, thrust and needle bearings |
| [BDS catalogue](https://help.bdsbearing.com/hubfs/Knowledge%20Base/General%20Bearings/General%20Bearing%20Catalog.pdf), [USA Roller Chain](https://usarollerchain.com/inch-ball-bearings-s/6198.htm) | inch bearings R |
| [Samick listing](https://aimsindustrial.com.au/products/samick-closed-linear-bushing-lm), [3DJake](https://www.3djake.com/brozzl/linear-bearings) | linear ball bushings |
| [Fabory](https://www.fabory.com/) product pages | two DIN 179 bushes |
| [PGB Europe](https://www.pgb-europe.com/) product pages | DIN 6334 M4, M5 |
| [RoyMech](https://roymech.org/Useful_Tables/Flanges/BSEN1092_16_Dimensions.html), [wermac.org](https://www.wermac.org/din/dim_wn_flg_pn16.html) | EN 1092-1 PN 16 weld neck and plate flanges, bolting |
| [wermac.org fittings](https://www.wermac.org/fittings/submenu_fittings.html), [pipes](https://www.wermac.org/pipes/dim_pipes.html) | ASME B16.9 elbows, tees, reducers, caps; STD walls of ASME B36.10 |
| [projectmaterials](https://blog.projectmaterials.com/flanges/flange-dimensions/en-1092-blind-flange-sizes), [HT Pipe](https://www.htpipe.com/tools/size-chart/en-1092-1-blind-flange-dimensions.html) | EN 1092-1 PN 16 blind flanges |

Corrected, a typed value against a source:

- DIN 933 and DIN 931 head heights (they are the ISO ones; only some across flats differ), ISO 10642 head heights.
- ISO 4162: every across flats, head height and flange diameter (they mixed two series; now those of EN 1662, derived from ISO 4162).
- ISO 8752 size 6 wall; DIN 660 sizes 3, 6 head; DIN 7349 sizes 10, 16, 20; DIN 705 sizes 6, 40, 50; bearing HK1210 outside diameter (16).
- ASME B18.3: socket depths of the cap screws, heads of the flat countersunk screws, head of the No.10 button screw. ASME B18.21.1: outside diameters.
- Lengths the standards do not list are gone from ISO 4026 to 4029, ISO 2338, DIN 7, 913 and 916 (an M12 x 4 set screw, with an 8 mm socket).
- `ISO 3601` was renamed `Metric O-Ring`: its round sizes are common trade sizes, not that series (sections 1.78, 2.62, 3.53, 5.33, 6.99).

Both values kept:

| Family | Rows | Why |
|---|---|---|
| DIN 557 | `M10` s 16 and `M10 (s 17)`; `M12` s 18 and `M12 (s 19)` | the page gives both across flats |
| DIN 434 | `M12` d1 13.5 and `M12 (d1 14)`; `M16` d1 17.5 and `M16 (d1 18)` | the page's two tables differ |
| ISO 4032 | `M6` 5.2 and `M6 (m 5)`; `M24` 21.5 and `M24 (m 19)` | fasten.it prints the heights DIN 934 has for these sizes |
| DIN 1587 | `M10` s 17 and `M10 (s 16)`; `M12` s 19 and `M12 (s 18)` | the page gives both across flats |
| ISO 4161 | `M10` s 15 and `M10 (s 16)` | ISO 4161 against DIN EN 1661 |
| ISO 7093-1 | `5` h 1.2 and `5 (h 1)`; `20` d1 22 and `20 (d1 21)` | a model listing and DIN 9021 against the fasten.it page |
| Series R | `R6`, `R8`, `R10` and a row with the narrower width | catalogues list two widths; which is open or shielded was not confirmed |
| EN 1092-1 Type 11 | a row per DN where the pages differ in `b`, hub `N`, length `H`, wall `S` (`DN20 (b 18 N 40 H 40)`); `DN65` and `DN65 (n 4)` | RoyMech against wermac.org; four holes at DN 65 by agreement |
| EN 1092-1 Type 05 | `DN150` b 22 and `DN150 (b 16)`, `DN200` and `DN200 (b 18 n 8)`, and most sizes likewise | projectmaterials against HT Pipe. DN 10 only in HT Pipe, DN 450 only in projectmaterials |
| DIN 444 B | `M27` d1 24 and `M27 (d1 25)`, likewise M30, M36, M39 | the page prints both holes |
| DIN 478 | `M16` s 16 and `M16 (s 17)`, `M20` s 21 and `M20 (s 22)` | the page gives a range |
| DIN 988 | shim thickness and the `s` of the page | the page lists one thickness per size, the shims come in several |

Other findings:

- ISO 4017 M30 / M36: 18.7 / 22.5 is the middle of the standard's limits; one page rounds them to 19 / 23.
- Set screws: socket depth `t` depends on the length; the tables hold the long screws' value.
- ASME B18.21.1 1/2: the page repeats the row of 5/8, so this size was not compared.

Types added 2026-11-03 (DIN 84, 85, 94, 434, 508, 557, 603, 661, 929, 937, 938, 981, 1444, 1473, 2093, 5406, 6319, 6797, 6799, 6886) were
written from the fasten.it pages, each `checked` cell saying so. Their shapes leave some details out for now, which the numbers do not depend
on: screw slots, a carriage bolt's square neck, castle slots, washer teeth, a taper washer's slope, a stud's plain middle, and a cotter
pin is a plain pin; `spec/6.3-content-center.md` §6.5b keeps the list.

DIN 315, 444, 478 and ISO 7379 give limits: the tables hold the largest value of each. DIN 7337's lengths are common trade lengths; a typing slip on its page (`K 1.1.0` at Ø5) is read as 1.10.

Compared only in part: HK2516 and HK3016 (the name gives bore and width, not the outside diameter), DIN 179 (sizes 6 and 12), SAE (1/4, 1/2) and USS (1/2) washers; ISO 2339, DIN 975 and 976, UNC rods: diameters and pitches, not lengths. ASME B18.2.2 lists no No.10 nut.

**Not compared:** Metric O-Ring, ISO 3547, DIN 3760, DIN 5419, Ring Magnet, ANSI B17.1: no readable public table was found.

Bearing boundary dimensions follow ISO 15, so makers agree on them.

**Not used:** MISUMI. Its terms for CAD data forbid obtaining it in bulk and reusing it beyond designing with its
products; its catalogue is not a source for this library unless MISUMI agrees.
