/* Three-valued evaluation: empire/event checks are unknown, not false. */
(() => {
  "use strict";
  function evaluate(condition, owned) {
    if (condition === null || condition === undefined) return null;
    if (typeof condition === "boolean") return condition;
    const [op, ...children] = condition;
    if (op === "dlc") return owned.has(children[0]);
    if (op === "not") {
      const value = evaluate(children[0], owned);
      return value === null ? null : !value;
    }
    const values = children.map(child => evaluate(child, owned));
    if (op === "all") return values.includes(false) ? false : values.includes(null) ? null : true;
    if (op === "any") return values.includes(true) ? true : values.includes(null) ? null : false;
    return null;
  }
  window.TechDlc = { evaluate, allows: (condition, owned) => evaluate(condition, owned) !== false };
})();
