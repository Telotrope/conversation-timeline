// Switches between the Calendar, Conversations, "Review & flags" and
// Analytics tabs: highlights the chosen tab button, shows that tab's view and
// hides the others.

// --- Tabs ---
export function switchTab(name){
  document.querySelectorAll('nav.tabs button').forEach(b=>{
    b.classList.toggle('active', b.dataset.tab === name);
  });
  document.querySelectorAll('.view').forEach(v=>{
    v.classList.toggle('active', v.id === 'view-' + name);
  });
}
