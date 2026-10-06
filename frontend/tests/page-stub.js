// A loose stand-in for the page, for testing the modules that draw it and
// ask the server, without a browser. getElementById makes any element it is
// asked for; querySelectorAll answers only the selectors a test has set
// (with select), so a test states which elements a module finds. Elements
// keep what is written to them (innerHTML, textContent, attributes,
// children) and the listeners added to them, which a test can fire. Setting
// textContent and reading innerHTML escapes &, < and >, as a browser does,
// so escapeHtml works.

const ESCAPES = { '&': '&amp;', '<': '&lt;', '>': '&gt;' };

export class StubElement {
  constructor(tag = 'div', id = ''){
    this.tagName = tag.toUpperCase();
    this.id = id;
    this.children = [];
    this.attributes = {};
    this.listeners = {};
    this.dataset = {};
    this.style = {};
    this.hidden = false;
    this.disabled = false;
    this.checked = false;
    this.value = '';
    this.classes = new Set();
    this.text = '';
    this.html = null;
    const classes = this.classes;
    this.classList = {
      add: (...cs) => cs.forEach((c) => classes.add(c)),
      remove: (...cs) => cs.forEach((c) => classes.delete(c)),
      toggle: (c, on) => (on ? classes.add(c) : classes.delete(c)),
      contains: (c) => classes.has(c),
    };
  }
  get className(){ return [...this.classes].join(' '); }
  set className(v){ this.classes.clear(); String(v).split(' ').filter(Boolean).forEach((c) => this.classes.add(c)); }
  get textContent(){ return this.text; }
  set textContent(v){ this.text = String(v); this.html = null; this.children = []; }
  get innerHTML(){ return this.html !== null ? this.html : this.text.replace(/[&<>]/g, (c) => ESCAPES[c]); }
  set innerHTML(v){ this.html = String(v); this.children = []; }
  setAttribute(k, v){ this.attributes[k] = String(v); }
  getAttribute(k){ return k in this.attributes ? this.attributes[k] : null; }
  appendChild(c){ this.children.push(c); return c; }
  append(...cs){ this.children.push(...cs); }
  replaceChildren(...cs){ this.children = cs; this.text = ''; this.html = null; }
  addEventListener(type, fn){ (this.listeners[type] ||= []).push(fn); }
  // Fires the listeners of `type` with an event whose target is `target`.
  fire(type, target = this, extra = {}){
    for(const fn of this.listeners[type] || []) fn({ target, preventDefault(){}, stopPropagation(){}, ...extra });
  }
  // Fires only the listener added last: a module that draws an element
  // again in a browser gets a new element each time, while a stand-in
  // keeps every listener ever added.
  fireLast(type, target = this){
    const all = this.listeners[type] || [];
    all[all.length - 1]({ target, preventDefault(){}, stopPropagation(){} });
  }
  closest(){ return null; }
  querySelectorAll(){ return []; }
}

// Installs the stand-in as `document`. Returns { el(id), select(selector,
// elements) }.
export function installPage(){
  const els = new Map();
  const selected = new Map();
  const el = (id) => {
    if(!els.has(id)) els.set(id, new StubElement('div', id));
    return els.get(id);
  };
  globalThis.document = {
    getElementById: el,
    createElement: (tag) => new StubElement(tag),
    querySelectorAll: (selector) => selected.get(selector) || [],
    querySelector: (selector) => (selected.get(selector) || [])[0] || null,
  };
  return { el, select: (selector, elements) => selected.set(selector, elements) };
}
