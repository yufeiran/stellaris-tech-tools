/* Hold-to-pan camera controls, independent of zoom and keyboard repeat rate. */
(() => {
  "use strict";
  const DIRECTIONS = { w: [0, 1], arrowup: [0, 1], s: [0, -1], arrowdown: [0, -1],
    a: [1, 0], arrowleft: [1, 0], d: [-1, 0], arrowright: [-1, 0] };
  class KeyboardPan {
    constructor({ onPan, canPan = () => true }) {
      this.onPan = onPan; this.canPan = canPan; this.keys = new Set(); this.frame = 0; this.lastTime = 0;
      window.addEventListener("keydown", event => this.keydown(event));
      window.addEventListener("keyup", event => { this.keys.delete(event.key.toLowerCase()); if (!this.keys.size) this.stop(); });
      window.addEventListener("blur", () => this.stop());
      document.addEventListener("visibilitychange", () => { if (document.hidden) this.stop(); });
      document.addEventListener("focusin", () => { if (this.editing()) this.stop(); });
    }
    editing() {
      const active = document.activeElement;
      return /^(INPUT|TEXTAREA|SELECT)$/.test(active?.tagName || "") || active?.isContentEditable || Boolean(active?.closest?.('[contenteditable]:not([contenteditable="false"]), [role="textbox"], [role="combobox"]'));
    }
    keydown(event) {
      const key = event.key.toLowerCase();
      if (!DIRECTIONS[key] || event.defaultPrevented || event.isComposing || event.ctrlKey || event.metaKey || event.altKey || this.editing() || document.hidden || !this.canPan()) return;
      event.preventDefault();
      if (this.keys.has(key)) return;
      const wasIdle = !this.keys.size;
      this.keys.add(key);
      if (wasIdle) {
        this.lastTime = performance.now();
        this.move(1 / 60); // Short taps move too, even if released before the next frame.
        this.frame = requestAnimationFrame(time => this.tick(time));
      }
    }
    move(seconds) {
      let x = 0, y = 0;
      for (const key of this.keys) { const direction = DIRECTIONS[key]; x += direction[0]; y += direction[1]; }
      const length = Math.hypot(x, y);
      if (length) this.onPan(x / length * 640 * seconds, y / length * 640 * seconds);
    }
    tick(time) {
      this.frame = 0;
      if (!this.keys.size || this.editing() || document.hidden || !this.canPan()) { this.stop(); return; }
      this.move(Math.min(Math.max((time - this.lastTime) / 1000, 0), .05)); this.lastTime = time;
      this.frame = requestAnimationFrame(next => this.tick(next));
    }
    stop() {
      this.keys.clear();
      if (this.frame) cancelAnimationFrame(this.frame);
      this.frame = 0; this.lastTime = 0;
    }
  }
  window.KeyboardPan = KeyboardPan;
})();
