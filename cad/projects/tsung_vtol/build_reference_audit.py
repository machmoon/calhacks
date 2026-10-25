"""Reproduce baseline dimension audit without altering any CAD outputs."""
import json,hashlib
from pathlib import Path
ROOT=Path(__file__).resolve().parents[3]
source=ROOT/'out/freecad-aircraft-structural/result.json';r=json.loads(source.read_text())
parts={p['name']:p for p in r['physical_parts']}
lo=[min(p['bbox_mm'][0][j] for p in parts.values()) for j in range(3)]
hi=[max(p['bbox_mm'][1][j] for p in parts.values()) for j in range(3)]
reddit='https://www.reddit.com/r/diydrones/comments/1l89dcr/i_made_a_3d_printed_vtol_that_can_fly_130_miles/'
report={
 'target':'Tsung Xu June 2025 printed VTOL; specific reference reconstruction, not a generic quadplane',
 'audited_date':'2026-10-24',
 'sources':[
  {'id':'author_reddit','url':reddit,'kind':'primary author post and comments','facts':['1200 mm span','770 mm length','2.55 kg reported weight','Selig 3024 airfoil','near elliptical planform','MG90s aileron servos','full advertised endurance/range not flown due to crash/battery damage']},
  {'id':'author_video','url':'https://www.youtube.com/watch?v=e7AIKGDrlgs','kind':'primary author video','visual_observation_provenance':'Parent agent visually inspected frames at 0:30 and 0:32; this audit does not claim calibrated photogrammetry','timestamp_ledger':{'0:30':'Overall wireframe layout observed by parent','0:32':'Fusion CAD view and curved wing planform observed by parent','0:42':'Large oval pod top opening, curved thin shell, longitudinal internal ridge/rib and wing shoulder; electronics and battery on bench. No calibrated thickness or internal dimensions.'},'observations':['curved/swept leading edge and relatively straight trailing edge','central oval nose pod tapering aft to pusher','two parallel booms and four lift rotors','inward-canted fins supporting a top horizontal stabilizer']},
  {'id':'author_hn','url':'https://news.ycombinator.com/item?id=44241278','kind':'primary author post','facts':['Amprius SA08 6S2P by Upgrade Energy','2807 AOS lift and cruise motors','Gemfan 7042 lift and cruise props','Holybro Tekko32 F4 45A lift ESC','Flycolor Raptor 5 45A cruise ESC','Speedybee F405 Wing','M10 GPS','PLA-Aero and PETG printed structure, CFRP booms/spars']},
  {'id':'author_availability','url':'https://www.linkedin.com/posts/tsungxu_vtol-drone-opensource-activity-7341227749249847298-evUR','kind':'primary author statement','finding':'Author stated he could not open-source the design, citing export restrictions. This records availability, not a legal conclusion.'},
  {'id':'author_cad_tool','url':'https://www.tsungxu.com/p/i-made-a-3d-printed-vtol-that-can/comments','kind':'primary author comments','finding':'Author identified Fusion as CAD software; no downloadable CAD release found in checked author sources or targeted GitHub searches.'},
  {'id':'airfoil_coordinates','url':'https://m-selig.ae.illinois.edu/ads/coord/s3024.dat','kind':'UIUC source coordinates','local':'references/s3024.dat','sha256':hashlib.sha256((Path(__file__).parent/'references/s3024.dat').read_bytes()).hexdigest()}],
 'published_constraints':{'span_mm':1200,'length_mm':770,'airfoil':'S3024','reported_weight_kg':2.55,'performance_status':'130-mile/3-hour advertised capability is not a completed full-range test'},
 'baseline':{'file':str(source.relative_to(ROOT)),'sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'bbox_mm':[lo,hi],'extents_mm':[b-a for a,b in zip(lo,hi)],'length_error_mm':hi[0]-lo[0]-770,'length_error_percent':100*((hi[0]-lo[0])/770-1),'physical_count':len(parts),'clashes':sum(p['clash'] for p in r['fit']['pairs']),'unmet_contacts':sum(p['floating'] for p in r['fit']['pairs'])},
 'mismatches':[
 {'feature':'Overall length','baseline':'973.953441 mm solid extent','target':'770 mm published; measurement convention not fully specified','action':'Set explicit reconstruction longitudinal envelope and check exported bounds; do not scale span with length'},
 {'feature':'Main wing','baseline':'Constant 209.9979 mm chord over full 1200 mm span; SD7062 study','target':'S3024; author-described near elliptical planform','action':'Use sourced S3024 sections, spanwise chord and leading-edge stations traced/inferred from reference; mark station numbers inferred'},
 {'feature':'Tail topology','baseline':'Horizontal stabilizer z -9.505 to -2.542 mm at fin bases; fins are uncanted with constant lateral centers','target':'Observed inverted-U with inward-canted fins and top horizontal stabilizer','action':'Parameterize base/top lateral stations and height; connect stabilizer at fin tops'},
 {'feature':'Internal hardware','baseline':'Generic box/cylinder representations and substituted catalog hardware','target':'Named reference hardware, exact placements/pack geometry unpublished','action':'Record author identities; do not claim photographed internals reconstructed from silhouette'},
 {'feature':'Pod','baseline':'Four-section hollow loft, length414.012 mm, width163.218 mm, height110.378 mm','target':'Observed oval nose and sharp aft taper; exact stations unpublished','action':'Build separately with explicit inferred stations; preserve original as separate engineering study'},
 {'feature':'Performance evidence','baseline':'reference.rs calls range/endurance-derived speed flown_speed_mps and describes figures as flown','target':'Author says full test not completed','action':'Treat those quantities as claims, never geometry or flight-validation evidence'}],
 'reconstruction_unknowns':['Exact chord/leading-edge distribution','Wing incidence, twist, dihedral and segmentation','Pod cross sections and internal walls','Boom diameter, spacing, endpoints and attachment details','Tail airfoil, exact cant, chord and height','Electronics placements, battery envelope, cable routing and cooling','Fasteners, tolerances and material properties'],
 'original_cad_release':{'found':False,'scope':'Author blog/comments, author LinkedIn posts, Reddit thread and targeted public web/GitHub search; absence of search result is not proof no private files exist'},
 'exact_reconstruction_claim':False
}
(Path(__file__).parent/'reference-audit.json').write_text(json.dumps(report,indent=2))
print(json.dumps(report['baseline']))
