const test = require('node:test');
const assert = require('node:assert/strict');
const pdf = require('../dist/pdf-export.js');
class Element {
  constructor(name) { this.name=name; this.attrs={}; this.children=[]; this.style={}; this.textContent=''; }
  setAttribute(k,v) { this.attrs[k]=v; }
  append(...children) { this.children.push(...children); }
  getContext() { return { font:'', measureText: s=>({width:Array.from(s).length*8}) }; }
}
const doc={createElementNS:(_,n)=>new Element(n),createElement:n=>new Element(n)};
const all = n => [n,...n.children.flatMap(all)];
const tech = (id,en,zh)=>({id,name:{en,zh},area:'physics',tier:4,category:'computing',categoryName:{en:'Computing'}});
test('export keeps full English, Chinese and IDs as SVG text, never raster captions',()=>{
  const long='A very long entirely new mod technology that must remain fully searchable';
  const graph={items:[{tech:tech('tech_unlisted_你好',long,'从未预置的模组科技'),x:150,y:100}],edges:[],bands:[]};
  const built=pdf.makeSvg(doc,graph);
  const nodes=all(built.svg),runs=nodes.filter(n=>n.name==='tspan').map(n=>n.textContent);
  assert.ok(runs.includes(long));assert.ok(runs.includes('从未预置的模组科技'));assert.ok(runs.includes('tech_unlisted_你好'));
  assert.equal(nodes.filter(n=>n.name==='image').length,0);
  assert.ok(nodes.some(n=>n.attrs.textLength==='252'));
  assert.equal(nodes.filter(n=>n.attrs['data-technology']).length,1);
});
test('filtered exports omit nodes and edges outside the supplied visible set',()=>{
  const graph={items:[{tech:tech('a','A','甲'),x:0,y:40},{tech:tech('b','B','乙'),x:390,y:40}],
    edges:[{from:'a',to:'b'},{from:'not_visible',to:'b'}],bands:[]};
  const built=pdf.makeSvg(doc,graph);
  const paths=all(built.svg).filter(n=>n.name==='path' && n.attrs['marker-end']);
  assert.equal(paths.length,1);assert.equal(built.nodes.length,2);
});
test('large and sparse layouts retain their aspect ratio within PDF page bounds',()=>{
  const p=pdf.plan([{tech:tech('a','A','甲'),x:9e5,y:9e6}]);
  assert.ok(p.pageWidth<=420);assert.ok(p.pageHeight<=4700.001);
  assert.ok(Math.abs(p.pageWidth/p.pageHeight - p.width/p.height)<1e-9);
  assert.throws(()=>pdf.plan([]));
  assert.throws(()=>pdf.plan([{tech:{id:'x'},x:NaN,y:0}]));
});
test('hostile source strings stay plain text, with no markup or links',()=>{
  const value='<script>alert(1)</script> & " quoted';
  const built=pdf.makeSvg(doc,{items:[{tech:tech('unlisted',value,'安全'),x:0,y:40}],edges:[],bands:[]});
  const nodes=all(built.svg);
  assert.ok(nodes.some(n=>n.textContent===value));
  assert.equal(nodes.filter(n=>n.name==='script'||n.name==='foreignObject'||n.name==='a').length,0);
});
