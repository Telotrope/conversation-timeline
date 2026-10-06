// The Describe page (plan docs/plans/2026-10-05-screen-flow.md §7d): who took
// part, how the conversations were held and how they were transcribed, for
// one of three subjects:
//   { kind: 'batch', uploadIds, notHere }  the files an upload just processed
//                                          (notHere: [{ file, reason }] for
//                                          the ones that failed or stopped)
//   { kind: 'file', uploadId }              one file, from the Files tab
//   { kind: 'conversation', conversationId } one conversation, with its start
//                                          and end, from the Conversations tab
// The answers start as what the server holds (the upload's guess until
// someone saves). Done saves them, marking them confirmed; Cancel leaves
// without saving. Which page comes next is the page flow's decision, reached
// through the handlers main.js connects.

import {
  MEDIUM_KINDS, PARTICIPANT_KINDS, TRANSCRIPTION_SERVICES, checkAnswers, mediumToAnswer,
  participantsToAnswers, spanToAnswer,
} from '../core/conversation-metadata.js';
import { formatDateTime } from '../core/format.js';
import { errorKindOf, errorStatusOf } from '../core/page-error.js';
import {
  ensureAuthToken, fetchConversationRecords, fetchUploads, saveConversationMetadata, saveFileMetadata,
  usesRealLogin,
} from '../infra/api-client.js';
import { setDescribeStatus, showDescribeReminder } from './widgets/status-indicators.js';

let HANDLERS = { done: () => {}, cancel: () => {}, ranOut: () => {} };
let SUBJECT = null;
// Each section: { uploadIds, files, conversationId, record, answers, errors }.
let SECTIONS = [];
let FILES = [];
let RECORDS = [];
let SEPARATE = false;

// handlers: { done(subject), cancel(subject), ranOut() }.
export function connectDescribe(handlers){
  HANDLERS = handlers;
}

function token(){
  return ensureAuthToken(document.getElementById('devLoginSub').value.trim());
}

// A failure: an expired sign-in goes back to Sign-in; anything else is shown.
function report(id, err){
  console.error(err);
  if(errorKindOf(err) === 'not_logged_in' && usesRealLogin()) return HANDLERS.ranOut();
  setDescribeStatus(id, { detail: err.message, status: errorStatusOf(err), error_kind: errorKindOf(err) });
}

// --- Opening a subject ---

export async function openDescribe(subject){
  SUBJECT = subject;
  SEPARATE = false;
  SECTIONS = [];
  setDescribeStatus(null);
  setButtonsBusy(false);
  showDescribeReminder(subject.notHere || []);
  document.getElementById('describeTitle').textContent =
    subject.kind === 'conversation' ? 'Describe this conversation' : 'Describe your files';
  render();
  try{
    const t = await token();
    [FILES, RECORDS] = await Promise.all([fetchUploads(t), fetchConversationRecords(t)]);
  } catch(err){
    return report('describe.load_failed', err);
  }
  SECTIONS = buildSections();
  render();
}

function sharedAnswer(files, field, toAnswer){
  const first = JSON.stringify(files[0][field]);
  return files.every((f) => f[field] && JSON.stringify(f[field]) === first) ? toAnswer(files[0][field]) : null;
}

function fileSection(files){
  return {
    uploadIds: files.map((f) => f.upload_id),
    files,
    answers: {
      participants: sharedAnswer(files, 'participants', participantsToAnswers),
      medium: sharedAnswer(files, 'medium', mediumToAnswer),
      span: null,
    },
    errors: {},
  };
}

function buildSections(){
  if(SUBJECT.kind === 'conversation'){
    const record = RECORDS.find((r) => r.conversation_id === SUBJECT.conversationId);
    if(!record) return [];
    return [{
      conversationId: record.conversation_id, record, files: [],
      answers: {
        participants: participantsToAnswers(record.participants),
        medium: mediumToAnswer(record.medium),
        span: spanToAnswer(record.span),
      },
      errors: {},
    }];
  }
  const ids = SUBJECT.kind === 'file' ? [SUBJECT.uploadId] : SUBJECT.uploadIds;
  // In the order the files were chosen, not the server's newest-first.
  const own = ids.map((id) => FILES.find((f) => f.upload_id === id))
    .filter((f) => f && f.conversation_count > 0);
  if(own.length === 0) return [];
  return SEPARATE ? own.map((f) => fileSection([f])) : [fileSection(own)];
}

// --- Drawing ---

function el(tag, props = {}, ...children){
  const node = document.createElement(tag);
  Object.assign(node, props);
  node.append(...children.filter((c) => c !== null && c !== undefined));
  return node;
}

function render(){
  const body = document.getElementById('describeBody');
  const parts = [];
  if(SUBJECT.kind === 'batch' && ownFileCount() > 1) parts.push(separateToggle());
  SECTIONS.forEach((section, i) => parts.push(renderSection(section, i)));
  if(SECTIONS.length === 0 && FILES.length) parts.push(nothingToDescribe());
  body.replaceChildren(...parts);
}

function ownFileCount(){
  return FILES.filter((f) => SUBJECT.uploadIds.includes(f.upload_id) && f.conversation_count > 0).length;
}

function nothingToDescribe(){
  return el('p', { className: 'hint', textContent:
    "These files brought no conversations of their own: everything in them was already here, so there is nothing new to describe." });
}

function separateToggle(){
  const box = el('input', { type: 'checkbox', checked: SEPARATE });
  box.addEventListener('change', () => setSeparate(box.checked));
  return el('label', { className: 'toggle-field separate-toggle' }, box, ' Describe each file separately');
}

// Ticked: one form per file, each starting from the answers already given
// (or the file's own where they varied). Unticked: back to one form, after
// asking, since the per-file differences are lost.
function setSeparate(separate){
  if(separate){
    const shared = SECTIONS[0];
    SEPARATE = true;
    SECTIONS = buildSections().map((s) => ({
      ...s,
      answers: {
        participants: clone(shared.answers.participants) || s.answers.participants,
        medium: clone(shared.answers.medium) || s.answers.medium,
        span: null,
      },
    }));
  } else {
    const differ = new Set(SECTIONS.map((s) => JSON.stringify(s.answers))).size > 1;
    if(differ && !window.confirm('Describe the files together? The differences between them will be lost.')){
      return render();
    }
    const first = SECTIONS[0].answers;
    SEPARATE = false;
    SECTIONS = buildSections();
    SECTIONS[0].answers = first;
  }
  render();
}

function clone(value){
  return value === null ? null : JSON.parse(JSON.stringify(value));
}

function renderSection(section, i){
  return el('div', { className: 'describe-section' },
    sectionHeading(section),
    participantsField(section),
    mediumField(section, i),
    transcriptionField(section),
    section.record ? spanField(section) : null,
    section.record ? null : conversationList(section));
}

function sectionHeading(section){
  if(section.record){
    return el('div', {},
      el('h3', { textContent: section.record.name || '(untitled)' }),
      el('p', { className: 'counts', textContent: `From ${section.record.source.file_name}` }));
  }
  const names = section.files.map((f) => f.file_name).join(', ');
  const title = section.files.length > 1 ? `These ${section.files.length} files` : names;
  return el('div', {},
    el('h3', { textContent: title }),
    el('p', { className: 'counts', textContent: section.files.map((f) => countsLine(f, section.files.length > 1)).join(' · ') }));
}

// A file's upload time and counts; named only when the section has
// several files, since a single file's name is already its heading.
function countsLine(f, named){
  const present = f.already_present
    ? `, ${f.already_present} already present${f.gained_messages ? ` (${f.gained_messages} gained messages)` : ''}`
    : '';
  const counts = `uploaded ${formatDateTime(f.uploaded_at)}, ${f.conversation_count} new conversation${f.conversation_count === 1 ? '' : 's'}${present}`;
  return named ? `${f.file_name}: ${counts}` : counts.charAt(0).toUpperCase() + counts.slice(1);
}

function errorNote(section, key){
  const message = section.errors[key];
  return message ? el('span', { className: 'field-error', textContent: message }) : null;
}

function field(label, ...content){
  return el('div', { className: 'describe-field' }, el('span', { className: 'field-label', textContent: label }), ...content);
}

// A field the described conversations disagree on, left as is until the
// user chooses to set it.
function variesNote(onSet){
  const button = el('button', { type: 'button', className: 'btn-secondary btn-small', textContent: 'Set for all' });
  button.addEventListener('click', onSet);
  return el('span', {}, el('span', { className: 'varies', textContent: 'Varies between conversations — left as is. ' }), button);
}

function participantsField(section){
  const a = section.answers;
  if(a.participants === null){
    return field('Participants', variesNote(() => {
      a.participants = [{ kind: 'human', name: '' }, { kind: 'claude', name: '' }];
      render();
    }));
  }
  const add = el('button', { type: 'button', className: 'btn-secondary btn-small', textContent: 'Add participant' });
  add.addEventListener('click', () => { a.participants.push({ kind: 'human', name: '' }); render(); });
  return field('Participants', ...a.participants.map((p, i) => participantRow(section, p, i)),
    errorNote(section, 'participants'), add);
}

function participantRow(section, p, i){
  const kind = el('select', {}, ...PARTICIPANT_KINDS.map((k) =>
    el('option', { value: k.value, textContent: k.label, selected: k.value === p.kind })));
  kind.addEventListener('change', () => { p.kind = kind.value; render(); });
  const named = PARTICIPANT_KINDS.find((k) => k.value === p.kind)?.named;
  const name = named ? el('input', { type: 'text', value: p.name, placeholder: p.kind === 'human' ? 'Name' : 'Which AI' }) : null;
  if(name) name.addEventListener('input', () => { p.name = name.value; });
  // No Remove: the file says who took part, and taking someone out of the
  // list would say nothing true about the conversation (the user,
  // 2026-10-06). Matching a file's names to people is deferred-problems
  // item 12.
  return el('div', { className: 'participant-row' }, kind, name, errorNote(section, `participant.${i}`));
}

function mediumField(section, i){
  const a = section.answers;
  const choices = MEDIUM_KINDS.map((k) => {
    const radio = el('input', { type: 'radio', name: `medium-${i}`, value: k.value, checked: a.medium?.kind === k.value });
    radio.addEventListener('change', () => {
      a.medium = { kind: k.value, service: a.medium?.service || '', serviceName: a.medium?.serviceName || '' };
      render();
    });
    return el('label', { className: 'medium-choice' }, radio,
      el('span', {}, k.label, el('span', { className: 'explain', textContent: k.explain })));
  });
  const note = a.medium === null ? el('span', { className: 'varies', textContent: 'Varies between conversations — left as is unless you choose.' }) : null;
  return field('Kind of conversation', note, ...choices, errorNote(section, 'medium'));
}

function transcriptionField(section){
  const m = section.answers.medium;
  if(!m || !MEDIUM_KINDS.find((k) => k.value === m.kind)?.voice) return null;
  const select = el('select', {}, el('option', { value: '', textContent: 'Choose…' }),
    ...TRANSCRIPTION_SERVICES.map((s) => el('option', { value: s.value, textContent: s.label, selected: s.value === m.service })));
  select.addEventListener('change', () => { m.service = select.value; render(); });
  const other = m.service === 'other' ? el('input', { type: 'text', value: m.serviceName, placeholder: 'Which service' }) : null;
  if(other) other.addEventListener('input', () => { m.serviceName = other.value; });
  return field('Transcription', select, other, errorNote(section, 'service'), errorNote(section, 'serviceName'));
}

function spanField(section){
  const span = section.answers.span;
  const input = (key) => {
    const box = el('input', { type: 'datetime-local', value: span[key] });
    box.addEventListener('input', () => { span[key] = box.value; });
    return box;
  };
  return field('Start and end (your time zone)',
    el('div', { className: 'span-fields' }, input('start'), ' to ', input('end')), errorNote(section, 'span'));
}

// The section's conversations, by name and time range; collapsed, since
// one file can hold hundreds.
function conversationList(section){
  const mine = RECORDS.filter((r) => section.uploadIds.includes(r.source.upload_id));
  const items = mine.map((r) => el('li', { textContent:
    `${r.name || '(untitled)'} — ${formatDateTime(r.span.start)} to ${formatDateTime(r.span.end)}` }));
  return el('details', {},
    el('summary', { textContent: `Dates and times of ${mine.length} conversation${mine.length === 1 ? '' : 's'}` }),
    el('ul', {}, ...items),
    el('p', { className: 'hint', textContent: "Change one conversation's details, including its start and end, from the Conversations tab." }));
}

// --- Done and Cancel ---

function setButtonsBusy(busy){
  document.getElementById('describeSaveBtn').disabled = busy;
  document.getElementById('describeLeaveBtn').disabled = busy;
}

// Checks every section, marks what's wrong, and otherwise saves each one,
// then hands over to the page flow.
export async function describeDone(){
  const checks = SECTIONS.map((s) => checkAnswers(s.answers));
  SECTIONS.forEach((s, i) => { s.errors = checks[i].errors; });
  if(checks.some((c) => c.edit === null)){
    render();
    return setDescribeStatus('describe.invalid');
  }
  setDescribeStatus(null);
  setButtonsBusy(true);
  try{
    await saveSections(await token(), checks.map((c) => c.edit));
  } catch(err){
    setButtonsBusy(false);
    return report('describe.save_failed', err);
  }
  HANDLERS.done(SUBJECT);
}

async function saveSections(t, edits){
  for(const [i, section] of SECTIONS.entries()){
    if(Object.keys(edits[i]).length === 0) continue;
    if(section.record) await saveConversationMetadata(t, section.conversationId, edits[i]);
    else for(const id of section.uploadIds) await saveFileMetadata(t, id, edits[i]);
  }
}

export function describeCancel(){
  HANDLERS.cancel(SUBJECT);
}

