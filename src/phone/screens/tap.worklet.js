// The phone's microphone on the audio thread (1.7): its samples handed over about every 20 ms
// (the audio thread works 128 at a time). Loaded by voice.ts as a file of the app's own.
registerProcessor(
  'vyro-tap',
  class extends AudioWorkletProcessor {
    constructor() {
      super();
      this.block = new Float32Array(960);
      this.n = 0;
    }

    process(inputs) {
      const c = inputs[0] && inputs[0][0];
      if (c) {
        for (let i = 0; i < c.length; i++) {
          this.block[this.n++] = c[i];
          if (this.n === this.block.length) {
            this.port.postMessage(this.block.slice(0));
            this.n = 0;
          }
        }
      }
      return true;
    }
  },
);
