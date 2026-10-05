// The settings that point the page at a deployed backend and its Cognito
// login, written from the deployed stack's outputs by
// scripts/write-deploy-config.sh (migration plan §V2e, E5).
//
// Parsed once, here, into checked values: an address the page will send
// you to for signing in, or send your login token to, must be what it
// claims to be. Plain http is accepted only for this machine (localhost and
// 127.0.0.1), which is what the browser tests' pretend Cognito uses.

// A deployment's name goes into the settings file's path, so only plain
// names are accepted: lowercase letters, digits and hyphens.
export function isDeployName(name){
  return typeof name === 'string' && /^[a-z0-9-]{1,32}$/.test(name);
}

// A version as `git describe --always --dirty` writes it, e.g. 1a2b3c4-dirty.
const PAGE_VERSION = /^[0-9A-Za-z._-]{1,64}$/;

function checkedAddress(value, field){
  let url;
  try{ url = new URL(value); } catch(e){
    throw new Error(`deployment settings: ${field} is not a web address: ${JSON.stringify(value)}`);
  }
  const local = url.hostname === 'localhost' || url.hostname === '127.0.0.1';
  if(url.protocol !== 'https:' && !(url.protocol === 'http:' && local)){
    throw new Error(`deployment settings: ${field} must use https: ${JSON.stringify(value)}`);
  }
  if(url.search || url.hash){
    throw new Error(`deployment settings: ${field} must not have a query or fragment: ${JSON.stringify(value)}`);
  }
  return url.href.replace(/\/+$/, '');
}

// text: the settings file's contents. Returns { apiBase, cognitoDomain,
// clientId, recordActivity? }, addresses without a trailing slash; throws
// naming the first field that's missing or wrong.
//
// recordActivity (optional): whether the page records your activity (plan
// docs/plans/completed/2026-10-02-activity-instrumentation.md §4). Absent means off.
// pageVersion (optional): the page code's version (its commit, from `git
// describe`), carried on every activity record; absent means unknown.
// Each is returned only when the file has it, so a settings file without
// them reads back exactly as before; readers treat absent as off / unknown.
export function parseDeployConfig(text){
  let raw;
  try{ raw = JSON.parse(text); } catch(e){
    throw new Error(`deployment settings are not JSON: ${e.message}`);
  }
  if(raw === null || typeof raw !== 'object' || Array.isArray(raw)){
    throw new Error('deployment settings must be a JSON object');
  }
  for(const field of ['apiBase', 'cognitoDomain', 'clientId']){
    if(typeof raw[field] !== 'string' || raw[field] === ''){
      throw new Error(`deployment settings: ${field} is missing`);
    }
  }
  if(!/^[A-Za-z0-9]+$/.test(raw.clientId)){
    throw new Error(`deployment settings: clientId has unexpected characters: ${JSON.stringify(raw.clientId)}`);
  }
  if('recordActivity' in raw && typeof raw.recordActivity !== 'boolean'){
    throw new Error(`deployment settings: recordActivity must be true or false: ${JSON.stringify(raw.recordActivity)}`);
  }
  if('pageVersion' in raw && !(typeof raw.pageVersion === 'string' && PAGE_VERSION.test(raw.pageVersion))){
    throw new Error(`deployment settings: pageVersion must be 1-64 letters, digits, dots, underscores or hyphens: ${JSON.stringify(raw.pageVersion)}`);
  }
  const parsed = {
    apiBase: checkedAddress(raw.apiBase, 'apiBase'),
    cognitoDomain: checkedAddress(raw.cognitoDomain, 'cognitoDomain'),
    clientId: raw.clientId,
  };
  if('recordActivity' in raw) parsed.recordActivity = raw.recordActivity;
  if('pageVersion' in raw) parsed.pageVersion = raw.pageVersion;
  return parsed;
}
