import hashlib,json,os,subprocess,sys,tempfile,unittest
from pathlib import Path
import pdf_input
ROOT=Path(__file__).resolve().parent
FIXTURES=ROOT/'fixtures/pdf'
class PdfInputTests(unittest.TestCase):
 def setUp(self):self.temp=tempfile.TemporaryDirectory();self.root=Path(self.temp.name)
 def tearDown(self):self.temp.cleanup()
 def prepare(self,name='vtol-requirements.pdf'):return pdf_input.prepare(FIXTURES/name,self.root/'prepared')
 def test_real_nondefault_pdf_all_pages_and_immutable_source(self):
  source=FIXTURES/'vtol-requirements.pdf';before=source.read_bytes();m=self.prepare();self.assertEqual(m['pages'],2);self.assertEqual(len(m['page_images']),2);self.assertEqual(m['id'],hashlib.sha256(before).hexdigest())
  text=Path(m['text_path']).read_text()
  for value in ['1900','85','32']:self.assertIn(value,text)
  self.assertEqual(source.read_bytes(),before);self.assertEqual(Path(m['source_path']).read_bytes(),before);self.assertEqual(m['trust'],'untrusted_source_material');self.assertLessEqual(m['images_total_bytes'],16*1024*1024)
  self.assertTrue(all(Path(p).read_bytes().startswith(b'\x89PNG') for p in m['page_images']));self.assertEqual(m,pdf_input.verify_prepared(self.root/'prepared',m['id']))
 def test_image_only_is_rasterized_not_silently_blank(self):
  m=self.prepare('image-only.pdf');self.assertEqual(m['text_characters'],0);self.assertEqual(len(m['page_images']),1);self.assertFalse(m['per_page'][0]['has_extractable_text']);self.assertTrue(any('OCR' in x for x in m['limitations']));self.assertGreater(m['per_page'][0]['embedded_image_count'],0)
  from PIL import Image
  with Image.open(m['page_images'][0]) as im:self.assertLess(min(low for low,high in im.getextrema()),200);self.assertLessEqual(max(im.size),1200)
 def test_encrypted_rejected(self):
  with self.assertRaises(pdf_input.PdfError) as cm:self.prepare('encrypted.pdf')
  self.assertEqual(cm.exception.code,'encrypted_pdf')
 def test_page_limit_rejects_without_truncation(self):
  with self.assertRaises(pdf_input.PdfError) as cm:self.prepare('five-pages.pdf')
  self.assertEqual(cm.exception.code,'page_limit')
 def test_malformed_rejected(self):
  p=self.root/'bad.pdf';p.write_bytes(b'%PDF-1.7\nnot a valid object tree\n%%EOF')
  with self.assertRaises(pdf_input.PdfError) as cm:pdf_input.prepare(p,self.root/'out')
  self.assertEqual(cm.exception.code,'malformed_pdf')
 def test_oversize_rejected_before_parser(self):
  p=self.root/'big.pdf'
  with p.open('wb') as f:f.write(b'%PDF-1.7');f.seek(pdf_input.MAX_BYTES);f.write(b'x')
  with self.assertRaises(pdf_input.PdfError) as cm:pdf_input.prepare(p,self.root/'out')
  self.assertEqual(cm.exception.code,'source_size_limit')
 def test_cached_text_tampering_is_rejected(self):
  m=self.prepare();Path(m['text_path']).write_text('Span: 10000 mm')
  with self.assertRaises(pdf_input.PdfError) as cm:self.prepare()
  self.assertEqual(cm.exception.code,'invalid_cache')
 def test_output_not_overwritten(self):
  out=self.root/'out';out.mkdir();(out/'keep').write_text('keep')
  with self.assertRaises(pdf_input.PdfError):pdf_input.prepare(FIXTURES/'vtol-requirements.pdf',out)
  self.assertEqual((out/'keep').read_text(),'keep')
 def test_cli_empty_directory_contract(self):
  out=self.root/'out';out.mkdir();p=subprocess.run([sys.executable,str(ROOT/'pdf_input.py'),'prepare',str(FIXTURES/'vtol-requirements.pdf'),str(out),'--name','Uploaded VTOL.pdf'],capture_output=True,text=True,timeout=60)
  self.assertEqual(p.returncode,0,p.stderr+p.stdout);m=json.loads(p.stdout);self.assertEqual(m,json.loads((out/'manifest.json').read_text()));self.assertEqual(m['name'],'Uploaded VTOL.pdf')
if __name__=='__main__':unittest.main()
