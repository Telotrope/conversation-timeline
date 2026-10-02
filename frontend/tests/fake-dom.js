// A small stand-in for page elements, for testing code that is handed
// elements (core/activity-event.js, ui/activity-listeners.js) without a
// browser. It supports only what that code uses: tagName, id, className,
// dataset, getAttribute, textContent, contains, and closest() with
// comma-separated selectors of the forms `tag`, `#id`, `.class` and
// `[attribute]`. Anything else in a selector fails loudly, so a test can't
// pass by a selector silently matching nothing.

function camel(name){
  return name.replace(/-([a-z])/g, (_, c) => c.toUpperCase());
}

export class FakeElement {
  constructor(tag, props = {}, children = []){
    this.tagName = tag.toUpperCase();
    this.id = props.id || '';
    this.className = props.className || '';
    this.attrs = props.attrs || {};
    this.ownText = props.text || '';
    for(const key of ['type', 'value', 'checked', 'files']){
      if(key in props) this[key] = props[key];
    }
    this.parent = null;
    this.children = children;
    for(const child of children) child.parent = this;
  }

  get dataset(){
    const out = {};
    for(const [k, v] of Object.entries(this.attrs)){
      if(k.startsWith('data-')) out[camel(k.slice(5))] = v;
    }
    return out;
  }

  getAttribute(name){
    if(name === 'id') return this.id || null;
    return name in this.attrs ? this.attrs[name] : null;
  }

  get textContent(){
    return this.ownText + this.children.map((c) => c.textContent).join('');
  }

  matchesOne(sel){
    if(sel.startsWith('#')) return this.id === sel.slice(1);
    if(sel.startsWith('.')) return this.className.split(' ').includes(sel.slice(1));
    const attr = /^\[([a-z-]+)\]$/.exec(sel);
    if(attr) return this.getAttribute(attr[1]) !== null;
    if(/^[a-z]+$/.test(sel)) return this.tagName === sel.toUpperCase();
    throw new Error(`fake-dom: unsupported selector ${JSON.stringify(sel)}`);
  }

  matches(selectors){
    return selectors.split(',').map((s) => s.trim()).some((s) => this.matchesOne(s));
  }

  closest(selectors){
    for(let node = this; node; node = node.parent){
      if(node.matches(selectors)) return node;
    }
    return null;
  }

  contains(other){
    for(let node = other; node; node = node.parent){
      if(node === this) return true;
    }
    return false;
  }
}

export function el(tag, props = {}, ...children){
  return new FakeElement(tag, props, children);
}
