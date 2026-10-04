"""Independently reconcile the sample against exact licensed source records."""
import argparse,csv, hashlib, json, re, sys, xml.etree.ElementTree as ET
from pathlib import Path
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--data',type=Path,required=True)
parser.add_argument('--sample',type=Path,required=True)
parser.add_argument('--inputs',type=Path,required=True)
parser.add_argument('--release-scripts',type=Path,required=True)
parser.add_argument('--receipt',type=Path,required=True)
args=parser.parse_args()
sys.path.insert(0,str(args.release_scripts.resolve()))
from validate_release import check_privacy
from release_gate import check_secrets
root=args.sample.resolve()
data=args.data.resolve()
inputs=json.loads(args.inputs.read_text())
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
for item in inputs['sources']:assert sha(data/'raw'/item['file'])==item['sha256']
seeds=set(inputs['genes']);hgnc={}
with (data/'raw/hgnc_complete_set.txt').open(encoding='utf-8') as f:
 for row in csv.DictReader(f,delimiter='\t'):
  if row['symbol'] in seeds and row['status']=='Approved':hgnc[row['symbol']]=row['hgnc_id']
expected={};names={}
for event,e in ET.iterparse(data/'raw/en_product1.xml',events=['end']):
 if e.tag=='Disorder':names['ORPHA:'+e.findtext('OrphaCode')]=e.findtext('Name');e.clear()
for event,e in ET.iterparse(data/'raw/en_product6.xml',events=['end']):
 if e.tag!='Disorder':continue
 disease='ORPHA:'+e.findtext('OrphaCode')
 for association in e.findall('DisorderGeneAssociationList/DisorderGeneAssociation'):
  gene=association.find('Gene');symbol=gene.findtext('Symbol')
  if symbol not in seeds:continue
  refs={r.findtext('Source'):r.findtext('Reference') for r in gene.findall('ExternalReferenceList/ExternalReference')}
  if 'HGNC:'+refs.get('HGNC','')!=hgnc[symbol]:continue
  key=f'{hgnc[symbol]}|gene_associated_with_condition|{disease}'
  expected[key]=(association.findtext('DisorderGeneAssociationType/Name'),association.findtext('DisorderGeneAssociationStatus/Name')=='Assessed')
 e.clear()
rows=json.loads((root/'cache/public-associations.json').read_text())['records']
assert len(rows)==16 and len({r['id'] for r in rows})==16
fields={'id','from','to','association','assessed','source_locator','source_sha256','source_url'}
for row in rows:
 assert set(row)==fields and expected[row['id']]==(row['association'],row['assessed'])
 assert row['source_sha256']==next(s['sha256'] for s in inputs['sources'] if s['file']=='en_product6.xml')
nodes=json.loads((root/'sample-nodes.json').read_text())
assert len(nodes)==15 and all(n['name']==names[n['id']] for n in nodes)
for n in nodes:
 for field in ['definition','parents','phenotypes','excluded','synonyms','related','prevalence','inheritance','onset','clinical_course']:
  assert not n[field]
 assert n['source_ids']==[n['id']]
for path in root.rglob('*'):
 if not path.is_file():continue
 check_privacy(path);check_secrets(path)
 value=path.read_bytes()
 assert not re.search(rb'[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}',value,re.I)
 assert b'BEGIN PRIVATE KEY' not in value
report={'format':'public-sample-source-audit-v1','status':'pass','sources':inputs['sources'],
    'conditions':15,'genes':4,'nodes':19,'gene_associations':16,'graph_edges':16,'identity_merges':0,
    'source_record_reconciliation':'all 16 association fields and 15 labels match the pinned source bytes',
    'privacy':'native sparse boundary plus current release privacy/secret scan on every sample file; no email addresses',
    'limitations':['original full source files not bundled','sample snapshots use source identifiers without disease merges',
        'no complete corpus reconstruction, nrese service, phenotype matching, contacts or model evaluation']}
args.receipt.write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:report[k] for k in ['status','conditions','genes','nodes','graph_edges','identity_merges']},indent=2))
