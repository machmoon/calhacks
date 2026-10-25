import concurrent.futures,hashlib,json,urllib.request,subprocess,tempfile
from pathlib import Path
P=Path(__file__).parent
sources={
'battery_cnhl':'https://chinahobbyline.com/collections/global-warehouse/products/cnhl-black-series-5000mah-14-8v-4s-65c-lipo-battery-with-xt90-plug',
'power_pm02':'https://holybro.com/products/pm02-v3-12s-power-module',
'AS150_manufacturer_drawing':'https://static.chipdip.ru/lib/654/DOC047654365.pdf',
'motor':'https://uav-en.tmotor.com/2025/Antigravity_0108/1268.html',
'lift_prop':'https://store.tmotor.com/product/polish-carbon-fiber-12x4-prop.html',
'cruise_motor':'https://store.tmotor.com/product/at2814-long-shaft-fixed-wing-motor.html',
'cruise_prop':'https://www.apcprop.com/product/11x5-5e/?v=84de8e2b14bb',
'prop_faq':'https://www.apcprop.com/faq/?v=84de8e2b14bb',
'esc':'https://www.hobbywing.com/en/uploads/file/20250930/64b726be7a56c9f415385f77683cdc46.pdf',
'controller':'https://holybro.com/products/pixhawk-6c-mini',
'controller_pinout':'https://docs.holybro.com/autopilot/pixhawk-6c-mini/system-diagram-and-pinout',
'power':'https://holybro.com/collections/power-modules-pdbs/products/pixhawk-4-power-module-pm07',
'gnss':'https://holybro.com/products/m10-gps',
'receiver':'https://radiomasterrc.com/products/rp3-expresslrs-2-4ghz-nano-receiver',
'telemetry':'https://holybro.com/products/sik-telemetry-radio-v3',
'airspeed':'https://www.mateksys.com/?portfolio=aspd-4525',
'servo':'https://www.hiteccs.com/public/uploads/data_sheet/HCS_HS-5065MG_Specsheetv2.2_-1729884119.pdf',
'servo_conflict':'https://hitecrcd.com/hs-5065mg-high-torque-metal-gear-feather-servo/',
'bec':'https://hobbywing.oss-cn-shenzhen.aliyuncs.com/pdf/pdfen/UBEC10A2-6S.pdf',
'battery':'https://genstattu.com/tattu-5200mah-14-8v-35c-4s1p-lipo-battery-pack-with-xt60-plug.html',
'spar':'https://www.easycomposites.co.uk/16mm-woven-finish-carbon-fibre-tube',
'spar_datasheet':'https://media.easycomposites.co.uk/datasheets/EC-TDS-Carbon-Fibre-Woven-Finish-Tube.pdf',
'pushrod':'https://www.dubro.com/products/2-56-threaded-rods-30-762-mm-6-pkg',
'eol':'https://www.mateksys.com/?page_id=2853'}
def get(item):
 k,u=item;r={'id':k,'url':u,'retrieved':'2026-10-24','primary':True}
 try:
  existing=list((P/'sources').glob(k+'.*'))
  if existing:
   data=existing[0].read_bytes();r.update(path=str(existing[0]),sha256=hashlib.sha256(data).hexdigest(),bytes=len(data),status='retrieved');return r
  proc=subprocess.run(['curl','-fLsS','--max-time','25','--max-filesize','3000000',u],capture_output=True,check=True);data=proc.stdout;r['resolved_url']=u
  if len(data)>3000000:raise ValueError('bounded3MBsource limit')
  ext='.pdf' if data.startswith(b'%PDF') else '.html';p=P/'sources'/(k+ext);p.write_bytes(data)
  r.update(path=str(p),sha256=hashlib.sha256(data).hexdigest(),bytes=len(data),status='retrieved')
 except Exception as e:r.update(status='fetch_failed',error=str(e),evidence='web tool inspected page; no local payload hash')
 return r
with concurrent.futures.ThreadPoolExecutor(max_workers=6) as ex:rows=list(ex.map(get,sources.items()))
(P/'source-manifest.json').write_text(json.dumps({'sources':rows},indent=2))
print([(r['id'],r['status']) for r in rows])
