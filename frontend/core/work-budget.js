// How long a loop may work before it stops to report (plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §8b, §8c). Every
// wait in the page is a loop that does one small step at a time and asks
// its budget, between steps, "may I do another?". The real budget answers by
// the clock; a test's budget answers by counting steps, so a test can stop a
// loop after exactly one, two or N steps. The loop's code is the same
// either way; only the budget handed to it differs.

// The page hands the screen back this often while drawing or computing.
export const TURN_MS = 50;
// The worker that prepares an upload reports this often.
export const REPORT_MS = 500;

// A budget that allows steps until `limitMs` have passed since it was made.
// `now` is the clock, in milliseconds.
export function clockBudget(limitMs, now = () => Date.now()){
  const started = now();
  return { allows: () => now() - started < limitMs };
}

// A budget that allows exactly `steps` more steps (tests).
export function stepBudget(steps){
  let left = steps;
  return {
    allows(){
      if(left <= 0) return false;
      left -= 1;
      return true;
    },
  };
}
