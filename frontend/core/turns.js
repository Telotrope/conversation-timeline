// Runs a long piece of page work in turns (plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §8b): drawing the
// timeline and the three analyses computed in the page. The work is a
// generator that yields after every step (one session drawn, one session
// counted) with how far it has got; each turn runs steps until its budget
// says stop, reports the last progress, and hands the screen back before
// the next turn. Every turn does at least one step, so the work always moves
// forward, however slow the computer.
//
// A generator stopped between turns can be handed in again later and
// carries on where it stopped, which is how an analysis left half-done
// resumes when you come back to it (§8c).

// steps: the generator. budget(): a fresh budget for one turn
// (core/work-budget.js). nextTurn(): resolves when the screen has been
// handed back. onProgress(value): the last value yielded in each turn.
// stopped(): checked before each turn; true leaves the generator where it is.
// Resolves to { finished: true, result } (the generator's return value), or
// { finished: false } when stopped.
export async function runInTurns(steps, { budget, nextTurn, onProgress = () => {}, stopped = () => false }){
  for(;;){
    if(stopped()) return { finished: false };
    const turn = budget();
    let next;
    do{
      next = steps.next();
      if(next.done) return { finished: true, result: next.value };
    } while(turn.allows());
    onProgress(next.value);
    await nextTurn();
  }
}
