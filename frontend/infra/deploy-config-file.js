// Which deployment the page talks to, and reading its settings file
// (migration plan §V2e, E5).
//
// Visiting timeline.html?deploy=<name> once points the page at
// frontend/deploy-configs/<name>.json, written by
// scripts/write-deploy-config.sh; the choice is remembered in this browser,
// the way ?api_base= is. ?deploy= with no name forgets it. With no
// deployment chosen the page uses the local backend and the dev login, and
// asks for no settings file at all.

import { isDeployName, parseDeployConfig } from '../core/deploy-config.js';

const REMEMBERED = 'timeline_deploy';

// The chosen deployment's name, or null for local development.
export function chosenDeployName(){
  const params = new URLSearchParams(window.location.search);
  if(params.has('deploy')){
    const name = params.get('deploy');
    try{
      if(name) localStorage.setItem(REMEMBERED, name);
      else localStorage.removeItem(REMEMBERED);
    } catch(e){ console.info('Could not remember the deployment choice:', e.message); }
    return name || null;
  }
  try{ return localStorage.getItem(REMEMBERED); } catch(e){ return null; }
}

// Fetches and checks the named deployment's settings.
export async function loadDeployConfig(name){
  if(!isDeployName(name)){
    throw new Error(`"${name}" is not a deployment name (lowercase letters, digits and hyphens)`);
  }
  const file = `frontend/deploy-configs/${name}.json`;
  const res = await fetch(file, { cache: 'no-store' });
  if(!res.ok){
    throw new Error(`could not read ${file} (${res.status}); run scripts/write-deploy-config.sh ${name}`);
  }
  return parseDeployConfig(await res.text());
}
