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
// clientId }, addresses without a trailing slash; throws naming the first
// field that's missing or wrong.
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
  return {
    apiBase: checkedAddress(raw.apiBase, 'apiBase'),
    cognitoDomain: checkedAddress(raw.cognitoDomain, 'cognitoDomain'),
    clientId: raw.clientId,
  };
}
