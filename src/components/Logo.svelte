<script lang="ts">
  // The Clokrix logo (2.2): the owner's shapes (assets/brand/clokrix-logo.svg) written out, so they
  // compile to plain markup; brand.ts has the same shapes for the share card's canvas, and
  // brand.test.ts keeps the two alike. The X alone, or the X and the word, each drawn magenta and
  // cyan offset either way under the main strokes, which take `--logo-ink` (white; near-black on a
  // light theme). `pixels`: the scatter (brand-scatter.ts), which the phone pages leave out.
  // Decorative: whatever shows it gives the name.
  interface Props {
    /** The word beside the X. */
    word?: boolean;
    pixels?: { mark: string; word: string } | null;
    class?: string;
  }
  let { word = true, pixels = null, class: cls = '' }: Props = $props();

  // brand.ts `VIEW`: the X alone, the X and the word, and those with the pixels.
  const view = $derived(word ? (pixels ? '48 40 534 116' : '48 40 528 110') : '48 39 112 112');
</script>

<svg class={cls} viewBox={view} aria-hidden="true" focusable="false">
  <g transform="translate(104,95) scale(0.72)" fill="none" stroke-width="30">
    <path d="M-58,-58 L58,58 M58,-58 L-58,58" stroke="#FF2E93" opacity="0.85" transform="translate(-7,-5)" />
    <path d="M-58,-58 L58,58 M58,-58 L-58,58" stroke="#22D3EE" opacity="0.85" transform="translate(7,5)" />
    <path d="M-58,-58 L58,58 M58,-58 L-58,58" stroke="var(--logo-ink, #fff)" />
    {#if word && pixels}{@html pixels.mark}{/if}
  </g>
  {#if word}
    <g transform="translate(215,67) scale(0.9)" fill="none" stroke-width="11" stroke-miterlimit="6">
      <path
        d="M48,4 L16,4 L4,16 L4,50 L16,62 L48,62 M68,4 L68,62 L108,62
          M124,16 L136,4 L160,4 L172,16 L172,50 L160,62 L136,62 L124,50 Z M192,4 L192,62
          M234,4 L194,34 M208,27 L236,62 M258,62 L258,4 L290,4 L298,12 L298,28 L290,36 L258,36
          M278,36 L300,62 M314,4 L330,4 M322,4 L322,62 M314,62 L330,62 M350,4 L390,62
          M390,4 L350,62"
        stroke="#FF2E93"
        opacity="0.85"
        transform="translate(-5,-3) skewX(-10)"
      />
      <path
        d="M48,4 L16,4 L4,16 L4,50 L16,62 L48,62 M68,4 L68,62 L108,62
          M124,16 L136,4 L160,4 L172,16 L172,50 L160,62 L136,62 L124,50 Z M192,4 L192,62
          M234,4 L194,34 M208,27 L236,62 M258,62 L258,4 L290,4 L298,12 L298,28 L290,36 L258,36
          M278,36 L300,62 M314,4 L330,4 M322,4 L322,62 M314,62 L330,62 M350,4 L390,62
          M390,4 L350,62"
        stroke="#22D3EE"
        opacity="0.85"
        transform="translate(5,3) skewX(-10)"
      />
      <path
        d="M48,4 L16,4 L4,16 L4,50 L16,62 L48,62 M68,4 L68,62 L108,62
          M124,16 L136,4 L160,4 L172,16 L172,50 L160,62 L136,62 L124,50 Z M192,4 L192,62
          M234,4 L194,34 M208,27 L236,62 M258,62 L258,4 L290,4 L298,12 L298,28 L290,36 L258,36
          M278,36 L300,62 M314,4 L330,4 M322,4 L322,62 M314,62 L330,62 M350,4 L390,62
          M390,4 L350,62"
        stroke="var(--logo-ink, #fff)"
        transform="skewX(-10)"
      />
      {#if pixels}<g transform="skewX(-10)">{@html pixels.word}</g>{/if}
    </g>
  {/if}
</svg>
