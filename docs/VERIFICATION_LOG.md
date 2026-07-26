# Reference Verification Log — PROBLEM_CATALOG.md

This log documents the independent re-verification of all 50 references in
`docs/PROBLEM_CATALOG.md`, performed in response to an external hallucination
audit. It is the evidence trail behind the §0.5 methodology statement.

## Method

A multi-agent verification campaign was run in two waves:

- **Wave 1** — six parallel verification sub-agents, each independently
  confirmed a small batch of references against authoritative indices
  (ACM DL, IEEE Xplore, USENIX proceedings, NDSS, Springer Link, SciTePress,
  arXiv, DBLP, and author institutional homepages). Each sub-agent returned
  the confirmed title, full author list, venue, year, and ≥2 evidence URLs
  per reference.
- **Wave 2** — the orchestrator independently re-confirmed the most
  consequential corrections (those where sub-agents contradicted the audit)
  and spot-checked additional "audit-verified" references.

The audit flagged 23 references; the campaign confirmed 22 of those
corrections, found the audit itself wrong on 2 "verified" references
([20], [43]), and found the audit's proposed corrections wrong on 2 more
([4] has a third author; [39] has different authors than the audit claimed).
One reference ([44]) was a total fabrication and was replaced.

## Corrections applied (28 reference entries changed)

| Ref | Error class | Correction |
|-----|-------------|------------|
| [2] | Fabricated 8-author list; dropped title prefix; missed venue | → Jangda, Powers, Berger, Guha; "Not So Fast: …"; USENIX ATC 2019 |
| [3] | Fabricated co-authors; wrong year/edition | → Solo Conrad Watt; 7th CPP 2018 |
| [4] | Fabricated 6-author list; wrong year | → Lehmann, Kinder, Pradel; 29th USENIX Security 2020 (audit wrongly said 2 authors; real count is 3) |
| [5] | Fabricated 5th author "Patterson" | → 7 authors: Rao, Georges, Legoupil, Watt, Pichon-Pharabod, Gardner, Birkedal; PLDI 2023 |
| [6] | Wrong venue | → WWW 2021 (not ECOOP) |
| [16] | Wrong venue | → ESEC/FSE 2011 (not ECOOP) |
| [19] | Wrong venue | → ICSE 2017 (not OOPSLA) |
| [20] | Wrong venue (audit missed this) | → ICSE 2016 (not ASE 2016) |
| [21] | Wrong year/track | → ICSE 2015, Vol. 2, Poster track (not 2016 NIER) |
| [23] | Truncated title | → "Don't Call Us, We'll Call You: Characterizing Callbacks in JavaScript" |
| [24] | Wrong first author, year, venue | → Ocariza Jr., Pattabiraman, Zorn; IEEE ISSRE 2011 (not Gallaba 2018) |
| [27] | Wrong venue | → ICWE 2008 (not ICSM) |
| [28] | Wrong venue; spurious 3rd author | → Mesbah & van Deursen only; ICSE 2009 (not ICST; Roest dropped) |
| [29] | Omitted venue | → ICST 2008 |
| [32] | Wrong year | → WEBIST 2017 (not 2021) |
| [34] | Wrong first-author initial; truncated title | → Shujiang Wu et al.; "…in Web Browsers" (not K. Wu) |
| [35] | Title normalized to ACM DL form | → "A Reality Check of Browser-Based GPU Acceleration" |
| [36] | Misleading "et al." | → Solo Igor Santos-Grueiro |
| [39] | Wrong authors, venue, year (audit's correction was also wrong) | → Sharif, Chintalapati, Wobbrock, Reinecke; ASSETS 2021 (not Elavsky/Fan/Reinecke; not CHI 2022) |
| [40] | Missing 3rd author | → added Alida T. Muongchan; order Sharif, Wang, Muongchan, Reinecke, Wobbrock; CHI 2022 |
| [43] | Wrong venue (audit missed this) | → DIMVA 2020 (not MSR 2020) |
| [44] | Total fabrication — cited author set never co-authored a JS-bloat paper | → REPLACED with Liu, Tiwari, Bogdan, Baudry; "Detecting and removing bloated dependencies in CommonJS packages"; JSS 2025; arXiv:2405.17939 |
| [46] | Wrong 2nd-author initial | → A. S. Rabkin (not J.) |
| [47] | Wrong 4th author | → Martin Johns (not Johansson) |
| [48] | Completely wrong author list | → Davis, Williamson, Lee; 27th USENIX Security 2018 (not Schwarz/Lackner/Gruss) |
| [50] | Wrong venue and year | → Rokicki, Maurice, Laperdrix; EuroS&P 2021 (not IEEE S&P 2024) |

## References confirmed correct (no change)

[1], [7], [8], [9], [10], [11], [12], [13], [14], [15], [17], [18], [22],
[25], [26], [30], [31], [33], [37], [38], [41], [42], [45], [49].

## In-text prose fixes

- P3.4: "Gallaba et al." → "Gallaba, Beschastnikh & Mesbah … Ocariza, Pattabiraman & Zorn"
- P4.3: "Schwarz et al.'s 'A Sense of Time'" → "Davis, Williamson & Lee's 'A Sense of Time…'"; "Rokicki et al." → "Rokicki, Maurice & Laperdrix"
- P4.4: "Watt et al.'s binary-security study" → "Lehmann, Kinder & Pradel's binary-security study"
- P4.4 (WASM+GPU): "~1.55× native" → "roughly 1.5× slower than native" with "Not So Fast" framing correction
- P6.1: "Elavsky et al." → "Sharif, Chintalapati, Wobbrock & Reinecke"; VoxLens authors expanded
- P7.4: "Jangda et al." → "Jangda, Powers, Berger & Guha"; "1.55× native" → "1.5× slower"
- P9.3: "Soto-Valero, Durieux, Harrand & Barais" → "Liu, Tiwari, Bogdan & Baudry"
- P10.3: "Santos-Grueiro et al." → "Santos-Grueiro" (sole author)

## Evidence

Representative evidence URLs gathered by the verification sub-agents (full
JSON search-result captures were retained during the campaign):

- [2] https://www.usenix.org/conference/atc19/presentation/jangda ; https://arxiv.org/abs/1901.09056
- [3] https://dl.acm.org/doi/10.1145/3167082 ; https://www.cl.cam.ac.uk/~cpw25/publications/wasm-spec.pdf
- [4] https://www.usenix.org/conference/usenixsecurity20/presentation/lehmann ; https://dl.acm.org/doi/10.5555/3489212.3489225
- [5] https://dl.acm.org/doi/10.1145/3591230 ; https://iris-project.org/pdfs/iris-wasm.pdf
- [6] https://dl.acm.org/doi/10.1145/3442381.3450138 ; https://dblp.org/rec/conf/www/Hilbig0P21
- [16] https://dl.acm.org/doi/10.1145/2025113.2025125 ; https://cs.au.dk/~amoeller/papers/dom/paper.pdf
- [19] https://dl.acm.org/doi/10.1109/ICSE.2017.75 ; https://earlbarr.com/publications/typestudy.pdf
- [20] https://dl.acm.org/doi/10.1145/2884781.2884829 ; https://dblp.org/db/conf/icse/icse2016
- [24] https://ieeexplore.ieee.org/document/6132958 ; https://ece.ubc.ca/~frolino/projects/jser/tech_report.pdf
- [27] https://ieeexplore.ieee.org/document/4577876 ; https://research.tudelft.nl/en/publications/crawling-ajax-by-inferring-user-interface-state-changes
- [28] https://ieeexplore.ieee.org/document/5070522 ; https://ece.ubc.ca/~amesbah/resources/papers/icse09.pdf
- [32] https://www.scitepress.org/papers/2017/62348/62348.pdf ; https://researchportal.tuni.fi/en/publications/the-web-as-a-software-platform-ten-years-later
- [34] https://www.usenix.org/conference/usenixsecurity22/presentation/wu-shujiang ; https://dblp.org/pid/205/3035
- [39] https://dl.acm.org/doi/10.1145/3441852.3471202 ; https://faculty.washington.edu/wobbrock/pubs/chi-22.04.pdf (note: ASSETS '21)
- [40] https://dl.acm.org/doi/10.1145/3491102.3517431 ; https://faculty.washington.edu/wobbrock/pubs/chi-22.04.pdf
- [43] https://dl.acm.org/doi/10.1007/978-3-030-52683-2_2 ; https://dblp.org/pid/64/1932
- [44] https://www.sciencedirect.com/science/article/pii/S0164121225001773 ; https://arxiv.org/abs/2405.17939
- [46] https://dl.acm.org/doi/10.1145/2509136.2509515 ; https://2013.splashcon.org/details/oopsla-2013-papers/49/
- [47] https://www.usenix.org/conference/usenixsecurity15/technical-sessions/presentation/lekies ; https://dblp.org/pid/167/0532.html
- [48] https://www.usenix.org/conference/usenixsecurity18/presentation/davis ; https://dblp.org/db/conf/uss/uss2018
- [50] https://doi.org/10.1109/EuroSP51992.2021.00039 ; https://dblp.org/db/conf/eurosp/eurosp2021
