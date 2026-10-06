// Self-asserting smoke test for a mockup, run in the page itself.
//
//   open  apps/desktop/design-mockups/a-paper.html?selftest
//
// Drives the real editor through DOM events (typing, Esc, blur, Cmd+Enter, the gutter, Quick Add)
// and prints a pass/fail table to the console plus a small badge in the corner. It edits the
// buffer and puts it back, but reload without `?selftest` before you play with the page.
// Also runs headless in Node/jsdom (see the agent's harness) by calling TxSmoke.run() directly.
//
// Needs, on the page: [data-tx-main] on the main editor host, [data-tx-qa] on the Quick Add mount.
// Ref (dispatching synthetic events): https://developer.mozilla.org/en-US/docs/Web/API/EventTarget/dispatchEvent
(function () {
  const T = window.TodoTxt;
  const D = window.TxData;
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

  async function run() {
    const results = [];
    const check = (name, ok, detail = "") => results.push({ name, ok: !!ok, detail: ok ? "" : String(detail) });

    const host = document.querySelector("[data-tx-main]");
    if (!host) {
      check("page has [data-tx-main]", false);
      return report(results);
    }
    const input = host.querySelector(".tx-input");
    const count = (sel) => host.querySelectorAll(sel).length;
    const fire = (el, type, init = {}) => el.dispatchEvent(new (type === "blur" ? FocusEvent : type === "input" ? Event : KeyboardEvent)(type, { bubbles: true, ...init }));
    const type = (el, value) => {
      el.value = value;
      fire(el, "input");
    };
    const original = input.value;
    const docLines = D.DOC.split("\n").length;

    // -- rendering: every feature of the real FileView is present --
    check("one mirror line per doc line", count(".tx-line") === docLines, `${count(".tx-line")} vs ${docLines}`);
    check("7 priorities colored", count(".tok-priority") === 7, count(".tok-priority"));
    check("3 done lines dimmed", count(".tx-line.is-done") === 3, count(".tx-line.is-done"));
    check("2 ref: indicators", count(".tx-ref") === 2, count(".tx-ref"));
    check("100-char hint underlines the long line", count(".tx-long") >= 1);
    check('blank last line shows "Add a line…"', count(".tx-placeholder") === 1);
    check("dates, projects, contexts, tags colored", count(".tok-date") > 10 && count(".tok-project") > 8 && count(".tok-context") > 8 && count(".tok-tag-key") > 4);

    // -- typing, discard, commit --
    type(input, original.replace("buy milk", "buy oat milk"));
    check("typing updates the highlighted mirror", host.querySelector(".tx-mirror").textContent.includes("buy oat milk"));
    check("typing marks the buffer dirty", host.dataset.status === "dirty", host.dataset.status);
    fire(input, "keydown", { key: "Escape" });
    check("Esc discards back to the last saved text", input.value === original);
    check("Esc returns the status to clean", host.dataset.status === "clean", host.dataset.status);

    type(input, original.replace("buy milk", "buy oat milk"));
    fire(input, "blur");
    check("blur commits (saving)", host.dataset.status === "saving", host.dataset.status);
    await sleep(450);
    check("commit lands (saved)", host.dataset.status === "saved", host.dataset.status);
    type(input, original);
    fire(input, "blur");
    await sleep(450);

    // -- gutter: the round mark types "x <date> " for you (only on pages that have it) --
    const mark5 = host.querySelector('.tx-mark-btn[data-n="5"]');
    if (mark5) {
      mark5.click();
      check("gutter mark completes the line", T.completedInfo(input.value.split("\n")[4]) !== null, input.value.split("\n")[4]);
      host.querySelector('.tx-mark-btn[data-n="5"]').click();
      check("clicking again restores it", input.value === original, input.value.split("\n")[4]);
      fire(input, "blur");
      await sleep(450);
    }

    // -- Cmd+Enter opens the detail view for the caret line --
    input.setSelectionRange(0, 0);
    fire(input, "keydown", { key: "Enter", metaKey: true });
    await sleep(80);
    const parent = document.querySelector(".tx-d-parent .tx-mirror");
    check("Cmd+Enter opens detail with the parent line", parent && parent.textContent.includes("roadmap"));
    check("detail shows sub-list progress", document.body.textContent.includes("1 of 3 done"));
    check("detail sub-list has its 4 lines", document.querySelectorAll(".tx-d-list .tx-line").length === 4, document.querySelectorAll(".tx-d-list .tx-line").length);
    const back = document.querySelector(".tx-d-back");
    if (back) back.click();

    // -- Quick Add: lenient hint, Enter appends a date-stamped line --
    const qa = document.querySelector("[data-tx-qa]");
    const qin = qa && qa.querySelector(".tx-input");
    check("Quick Add is mounted", !!qin);
    if (qin) {
      type(qin, "(a) nope");
      await sleep(220);
      check("Quick Add hints (never blocks) on a bad priority", /uppercase/.test(qa.querySelector(".tx-qa-hint").textContent), qa.querySelector(".tx-qa-hint").textContent);
      type(qin, "call dentist +health");
      fire(qin, "keydown", { key: "Enter" });
      check("Enter appends a date-stamped task", input.value.includes(`${D.TODAY} call dentist +health\n`), input.value.slice(-80));
      type(input, original);
      fire(input, "blur");
      await sleep(450);
    }

    // Pages can add their own checks (c2 covers search, Universal, Settings).
    for (const fn of window.TxSmokeExtra || []) {
      try {
        await fn(check, { sleep, fire, type });
      } catch (e) {
        check("page-specific checks ran", false, e && e.stack ? e.stack.split("\n").slice(0, 3).join(" | ") : e);
      }
    }

    return report(results);
  }

  function report(results) {
    const failed = results.filter((r) => !r.ok);
    if (console.table) console.table(results);
    console.log(failed.length ? `SELFTEST FAILED (${failed.length}/${results.length})` : `SELFTEST OK (${results.length}/${results.length})`);
    if (document.body) {
      const b = document.createElement("div");
      b.id = "tx-selftest";
      b.textContent = failed.length ? `selftest: ${failed.length} failed — see console` : `selftest: ${results.length}/${results.length} ok`;
      b.style.cssText = `position:fixed;right:12px;bottom:52px;z-index:99999;padding:6px 12px;border-radius:999px;font:700 12px ui-monospace,monospace;color:#fff;background:${failed.length ? "#b91c1c" : "#15803d"}`;
      document.body.appendChild(b);
    }
    return results;
  }

  window.TxSmoke = { run };
  if (/[?&]selftest\b/.test(location.search)) window.addEventListener("load", () => setTimeout(run, 100));
})();
