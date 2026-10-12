<script lang="ts">
  import NotesKnowledgeCoverDesign from "./NotesKnowledgeCoverDesign.svelte";
  import { getEventColor } from "$lib/calendar/utils";
  import type { NotesCoverColor, NotesCoverDesign } from "$lib/notes/contracts/assets";
  import { notesCoverDesignBackground } from "$lib/notes/pages/cover";
  import { getTheme } from "$lib/stores/theme.svelte";
  import { resolveAppTokens } from "$lib/themes";

  let { pattern, color }: { pattern: NotesCoverDesign; color: NotesCoverColor } = $props();
  const id = $props.id();
  const theme = getTheme();
  const ink = $derived(color === "default" ? resolveAppTokens(theme.current)["--foreground"] : getEventColor(color, theme.current).bg);
  const deep = $derived(pattern === "contours"
    ? `color-mix(in srgb, ${ink} 65%, var(--foreground))`
    : `color-mix(in srgb, ${ink} 52%, black)`);
  const illustrated = $derived(pattern === "contours" || pattern === "studio" || pattern === "orbit" || pattern === "atlas");
</script>

<div
  class="relative size-full overflow-hidden"
  style={`background: ${notesCoverDesignBackground(pattern, ink)}; --cover-color: ${ink}; --cover-light: color-mix(in srgb, ${ink} 22%, var(--background)); --cover-mid: color-mix(in srgb, ${ink} 58%, var(--background)); --cover-deep: ${deep}; --cover-highlight: color-mix(in srgb, ${ink} 12%, white);`}
>
  {#if pattern === "study" || pattern === "mathematics" || pattern === "programming" || pattern === "finance"}
    <NotesKnowledgeCoverDesign {pattern} />
  {/if}
  {#if pattern === "mosaic"}
    <svg class="abstract-scene" viewBox="0 0 1200 240" preserveAspectRatio="none" aria-hidden="true">
      <defs>
        <pattern id={`${id}-weft`} width="6" height="6" patternUnits="userSpaceOnUse" patternTransform="rotate(28)">
          <path d="M0 0V6" stroke="var(--cover-highlight)" stroke-width="0.7" opacity="0.3" />
        </pattern>
        <pattern id={`${id}-grain`} width="13" height="13" patternUnits="userSpaceOnUse">
          <circle cx="2" cy="3" r="0.7" fill="var(--cover-deep)" opacity="0.3" />
          <circle cx="9" cy="10" r="0.5" fill="var(--cover-highlight)" opacity="0.55" />
        </pattern>
      </defs>
      <!-- Asymmetric print blocks share edges, arcs, and a continuous diagonal rhythm. -->
      <path d="M0 0H1200V240H0Z" fill="var(--cover-light)" />
      <path d="M0 0H234L92 240H0ZM456 0H747L600 240H314ZM984 0H1200V240H842Z" fill="var(--cover-mid)" />
      <path d="M0 153L234 0H355L160 240H0ZM747 0H855L687 240H600ZM1096 0H1200V98L1012 240H842Z" fill="var(--cover-color)" />
      <path d="M234 0H456V112H234ZM687 129H842V240H687ZM1012 0H1200V64H1012Z" fill="var(--cover-deep)" />
      <path d="M456 112A112 112 0 0 0 344 0V112ZM687 129A129 129 0 0 1 816 0H687ZM1012 240A176 176 0 0 1 1188 64H1200V240Z" fill="var(--cover-highlight)" />
      <path d="M344 112A112 112 0 0 1 456 224V112ZM456 240V112A128 128 0 0 1 584 240ZM816 0V129A129 129 0 0 0 945 0Z" fill="var(--cover-color)" />
      <path d="M0 0H150A150 150 0 0 1 0 150ZM160 240A94 94 0 0 1 254 146H314V240ZM945 0A96 96 0 0 1 1041 96H945Z" fill="var(--cover-deep)" />
      <path d="M0 0H92A92 92 0 0 1 0 92ZM456 240V154A86 86 0 0 1 542 240ZM1012 240A176 176 0 0 1 1188 64H1200V240Z" fill={`url(#${id}-weft)`} />
      <g fill="none" stroke="var(--cover-highlight)" stroke-width="1.1" opacity="0.65">
        {#each [0, 1, 2, 3, 4, 5] as line}
          <path d={`M${244 + line * 15} 112V${132 + line * 15}H314M${705 + line * 18} 240V${150 + line * 14}H842`} />
          <path d={`M0 ${132 - line * 15}A${132 - line * 15} ${132 - line * 15} 0 0 0 ${132 - line * 15} 0`} />
        {/each}
      </g>
      <g fill="none" stroke="var(--cover-deep)" stroke-width="1" opacity="0.4">
        <path d="M92 240L234 0M314 240L456 0M600 240L747 0M842 240L984 0" />
        <path d="M476 100H666M496 80H646M516 60H626M536 40H606M1063 219H1173M1081 199H1173M1100 179H1173M1119 159H1173" />
      </g>
      <path d="M0 0H1200V240H0Z" fill={`url(#${id}-grain)`} />
      <g fill="var(--cover-highlight)">
        <circle cx="297" cy="52" r="23" /><circle cx="766" cy="191" r="10" />
        <path d="M932 147H969V184H932Z" transform="rotate(30 950 165)" />
      </g>
    </svg>
  {/if}
  {#if pattern === "botanical" || pattern === "studio"}
    <svg class={pattern === "botanical" ? "nature-scene" : "absolute inset-0 size-full"} viewBox="0 0 1200 240" preserveAspectRatio="none" aria-hidden="true">
      {#if pattern === "botanical"}
        <!-- Flat ink layers form the whole landscape, including its sky and water. -->
        <defs>
          <pattern id={`${id}-rock-lines`} width="23" height="11" patternUnits="userSpaceOnUse" patternTransform="rotate(-12)">
            <path d="M0 1L15 0M7 8L21 7" fill="none" stroke="var(--cover-highlight)" stroke-width="0.65" opacity="0.3" />
          </pattern>
          <path id={`${id}-ridge`} d="M0 35L89 63 151 30 209 38 284 85 348 74 452 127 516 142 352 180 0 202Z" />
          <path id={`${id}-cliff`} d="M1200 27L1110 44 1064 32 1015 48 988 93 925 106 891 132 810 153 790 195 1200 224Z" />
        </defs>
        <path d="M0 0H1200V240H0Z" fill="var(--cover-light)" />
        <path d="M0 86L133 14 250 57 320 44 477 103 540 88 647 116 764 61 860 79 950 18 1047 51 1137 16 1200 46V184H0Z" fill="var(--cover-mid)" opacity="0.55" />
        <path d="M133 14L158 35 137 32 114 48 99 42ZM950 18L989 39 957 33 938 48 916 46Z" fill="var(--cover-highlight)" opacity="0.6" />
        <path d="M0 119L170 70 227 103 291 83 390 122 469 109 568 137 672 126 786 97 848 110 984 75 1091 103 1200 68V240H0Z" fill="var(--cover-mid)" />
        <path d="M551 111C602 109 666 119 654 135C641 153 516 151 505 173C494 193 661 185 723 209L820 240H290C368 220 472 214 449 199C429 186 452 168 526 160C631 149 628 133 594 128Z" fill="var(--cover-highlight)" />
        <path d="M565 117C599 120 635 129 614 139C584 153 492 153 480 173C468 193 617 196 659 212L716 240H441C480 225 519 219 494 207C453 188 475 176 543 167C654 153 659 131 565 117Z" fill="var(--cover-color)" opacity="0.35" />
        <use href={`#${id}-ridge`} fill="var(--cover-color)" />
        <use href={`#${id}-ridge`} fill={`url(#${id}-rock-lines)`} />
        <path d="M151 30L198 109 284 85 253 124 348 74 327 139 452 127 402 159 209 178 0 202V105L89 63 77 139Z" fill="var(--cover-deep)" opacity="0.35" />
        <use href={`#${id}-cliff`} fill="var(--cover-color)" />
        <use href={`#${id}-cliff`} fill={`url(#${id}-rock-lines)`} />
        <path d="M1110 44L1088 138 1015 48 988 93 1020 156 925 106 934 171 810 153 790 195 1200 224V118L1167 90Z" fill="var(--cover-deep)" opacity="0.4" />
        <g fill="none" stroke="var(--cover-highlight)" stroke-width="0.8" opacity="0.5">
          <path d="M89 63L151 30 209 38 284 85M348 74L452 127M1110 44L1064 32 1015 48 988 93M925 106L891 132 810 153" />
          <path d="M527 177H570M596 183H634M550 199H605M643 214H693M575 139H598M365 234H416" />
        </g>
        <path d="M0 178L77 162 163 179 238 164 324 184 392 177 437 195 388 209 329 221 292 240H0Z" fill="var(--cover-deep)" />
        <path d="M1200 179L1118 161 1053 175 993 160 912 177 867 169 792 190 715 197 764 213 824 240H1200Z" fill="var(--cover-deep)" />
        <g fill="var(--cover-deep)">
          {#each [[68, 169, 44], [100, 173, 32], [184, 176, 27], [350, 188, 20], [1124, 168, 43], [1088, 173, 31], [933, 180, 29], [855, 181, 23]] as tree}
            <path d={`M${tree[0]} ${tree[1] - tree[2]}l${-tree[2] * 0.19} ${tree[2] * 0.4}h${tree[2] * 0.1}l${-tree[2] * 0.22} ${tree[2] * 0.38}h${tree[2] * 0.24}v${tree[2] * 0.22}h${tree[2] * 0.14}v${-tree[2] * 0.22}h${tree[2] * 0.24}l${-tree[2] * 0.22} ${-tree[2] * 0.38}h${tree[2] * 0.1}Z`} />
          {/each}
        </g>
        <g fill="none" stroke="var(--cover-highlight)" stroke-width="0.75" opacity="0.4">
          <path d="M39 200L114 193 177 201M152 219L241 210 295 216M949 200L997 190 1073 202M1048 220L1150 215" />
        </g>
        <g fill="none" stroke="var(--cover-highlight)" stroke-width="1.1" opacity="0.65">
          <path d="M372 28H486M420 35H524M602 48H684M672 25H780M699 33H830" />
        </g>
        <path d="M534 53q6-5 12 0q6-5 12 0M576 39q4-4 8 0q4-4 8 0" fill="none" stroke="var(--cover-deep)" stroke-width="1" opacity="0.5" />
      {:else}
        <defs>
          <pattern id={`${id}-draft-grid`} width="28" height="28" patternUnits="userSpaceOnUse">
            <path d="M28 0H0V28" fill="none" stroke="var(--cover-mid)" stroke-width="0.6" />
          </pattern>
        </defs>
        <path d="M0 0H1200V240H0Z" fill={`url(#${id}-draft-grid)`} opacity="0.3" />
        <path d="M-90-30H407L167 265H-90ZM886-30H1280V117L1158 264H692Z" fill="var(--cover-mid)" opacity="0.14" />
        <path d="M-80 179C147 58 310 236 535 123S924 56 1280 163" fill="none" stroke="var(--cover-color)" stroke-width="46" opacity="0.08" />
        <g fill="none" stroke="var(--cover-color)" stroke-width="0.8" opacity="0.25">
          <path d="M-50 167C147 50 310 224 535 111S924 44 1280 151M-60 187C147 70 310 244 535 131S924 64 1280 171" />
          <path d="M16-25L204 264M82-25L270 264M973-25L1180 264M1001-25L1208 264" />
          <path d="M0 211H1200M0 32H1200" stroke-dasharray="2 8" />
        </g>
      {/if}
    </svg>
  {/if}
  {#if illustrated}
    <svg
      class={pattern === "contours" ? "size-full" : "cover-scene"}
      viewBox={pattern === "contours" ? "0 0 1200 400" : "0 0 1200 240"}
      preserveAspectRatio={pattern === "contours" ? "xMidYMid slice" : "xMidYMid meet"}
      aria-hidden="true"
    >
      <defs>
        <linearGradient id={`${id}-silk`} x1="0" y1="0" x2="1" y2="1">
          <stop stop-color="var(--cover-highlight)" />
          <stop offset="0.42" stop-color="var(--cover-mid)" />
          <stop offset="1" stop-color="var(--cover-deep)" />
        </linearGradient>
        <linearGradient id={`${id}-haze`} x1="0" y1="0" x2="0" y2="1">
          <stop stop-color="var(--cover-highlight)" stop-opacity="0.85" />
          <stop offset="1" stop-color="var(--cover-color)" stop-opacity="0.15" />
        </linearGradient>
        <linearGradient id={`${id}-night`} x1="0" y1="1" x2="1" y2="0">
          <stop stop-color="var(--cover-deep)" />
          <stop offset="0.55" stop-color="var(--cover-color)" />
          <stop offset="1" stop-color="var(--cover-mid)" />
        </linearGradient>
        <linearGradient id={`${id}-edge`} x1="0" y1="0" x2="0.7" y2="1">
          <stop stop-color="var(--cover-highlight)" stop-opacity="0.9" />
          <stop offset="0.45" stop-color="var(--cover-color)" stop-opacity="0.35" />
          <stop offset="1" stop-color="var(--cover-deep)" stop-opacity="0.05" />
        </linearGradient>
        <linearGradient id={`${id}-paper`} x1="0" y1="0" x2="1" y2="0.8">
          <stop stop-color="var(--cover-highlight)" />
          <stop offset="1" stop-color="var(--cover-light)" />
        </linearGradient>
        <linearGradient id={`${id}-crease`} x1="0" y1="0" x2="1" y2="0">
          <stop stop-color="var(--cover-deep)" stop-opacity="0.5" />
          <stop offset="1" stop-color="var(--cover-deep)" stop-opacity="0" />
        </linearGradient>
        <radialGradient id={`${id}-sphere`} cx="0.3" cy="0.25" r="0.75">
          <stop stop-color="var(--cover-highlight)" />
          <stop offset="0.48" stop-color="var(--cover-color)" />
          <stop offset="1" stop-color="var(--cover-deep)" />
        </radialGradient>
        <linearGradient id={`${id}-map-fade`}>
          <stop stop-color="white" stop-opacity="0" />
          <stop offset="0.18" stop-color="white" />
          <stop offset="0.82" stop-color="white" />
          <stop offset="1" stop-color="white" stop-opacity="0" />
        </linearGradient>
        <mask id={`${id}-map-field`} maskUnits="userSpaceOnUse" x="0" y="0" width="1200" height="240">
          <rect width="1200" height="240" fill={`url(#${id}-map-fade)`} />
        </mask>
        <clipPath id={`${id}-globe`}><circle cx="660" cy="120" r="86" /></clipPath>
      </defs>
      {#if pattern === "contours"}
        <circle cx="990" cy="60" r="185" fill={`url(#${id}-haze)`} />
        <circle cx="990" cy="60" r="212" fill="none" stroke="var(--cover-highlight)" stroke-opacity="0.22" />
        <path d="M-104 559C101 449 236 121 506 239C560 262.6 650.9375 315.0625 690.625 330.125C770 360.25 845 365 945 280S1020 5 1280-80L1280 600H-104Z" fill="var(--cover-color)" stroke="var(--cover-color)" opacity="0.14" stroke-width="110" />
        <g fill="none" stroke="var(--cover-highlight)" stroke-width="1.7" opacity="0.55">
          {#each [0, 1, 2, 3, 4, 5, 6, 7] as line}
            <path d="M-140 470C65 360 200 32 470 150S760 460 982 205 1150-15 1330-90" transform={`translate(${line * 24} ${line * 23})`} />
          {/each}
        </g>
        <g fill="var(--cover-highlight)" opacity="0.7">
          <circle cx="725" cy="72" r="3" /><circle cx="735" cy="72" r="1.5" /><circle cx="725" cy="82" r="1.5" />
        </g>
      {:else if pattern === "orbit"}
        <!-- The planet and its orbit belong to one field of light and trajectories. -->
        <path d="M-160 287C97 267 376 92 658 91S1005 99 1320-77" fill="none" stroke={`url(#${id}-edge)`} stroke-width="84" opacity="0.55" />
        <g transform="rotate(-12 650 118)" fill="none" stroke="var(--cover-highlight)">
          <ellipse cx="650" cy="118" rx="239" ry="48" stroke-width="0.7" opacity="0.24" />
          <ellipse cx="650" cy="118" rx="175" ry="36" stroke-width="1" opacity="0.5" />
          <ellipse cx="650" cy="118" rx="138" ry="28" stroke-width="8" opacity="0.1" />
        </g>
        <circle cx="650" cy="110" r="68" fill={`url(#${id}-sphere)`} />
        <path d="M609 58C625 51 647 50 670 62M590 78C623 64 666 68 696 87M583 102C622 93 677 108 714 122" fill="none" stroke="var(--cover-highlight)" stroke-width="2" opacity="0.12" />
        <path d="M517 145C527 187 760 130 783 85" fill="none" stroke="var(--cover-mid)" stroke-width="11" opacity="0.4" />
        <g fill="none" stroke="var(--cover-highlight)">
          <path d="M517 145C527 187 760 130 783 85" stroke-width="1.8" opacity="0.9" />
          <path d="M514 150C524 192 766 134 787 86" stroke-width="0.6" opacity="0.6" />
          <path d="M-160 289C106 265 370 102 659 98S1017 112 1320-61" stroke-width="0.7" opacity="0.25" />
          <path d="M-160 307C106 283 370 120 659 116S1017 130 1320-43" stroke-width="0.7" opacity="0.15" />
        </g>
        <circle cx="451" cy="158" r="12" fill={`url(#${id}-sphere)`} />
        <g fill="var(--cover-highlight)">
          {#each [[188, 92, 1], [253, 49, 1.5], [318, 113, 0.8], [365, 68, 1], [493, 38, 1.3], [736, 30, 0.8], [831, 139, 1.4], [911, 64, 0.8], [967, 180, 1], [1056, 108, 1.3], [1127, 42, 0.8]] as star}
            <circle cx={star[0]} cy={star[1]} r={star[2]} opacity="0.65" />
          {/each}
        </g>
        <path d="M188 92L253 49 365 68 318 113 188 92M318 113L451 158" fill="none" stroke="var(--cover-highlight)" stroke-width="0.6" opacity="0.14" />
      {:else if pattern === "atlas"}
        <!-- Map contours, routes, and the globe overlap on one continuous cartographic field. -->
        <g mask={`url(#${id}-map-field)`}>
        <g fill="none" stroke="var(--cover-mid)" stroke-width="0.7" opacity="0.35">
          {#each [0, 1, 2, 3, 4, 5] as line}
            <path d={`M80 ${30 + line * 37}Q620 ${-5 + line * 51} 1120 ${30 + line * 37}`} />
            <path d={`M${180 + line * 160}-10Q${100 + line * 190} 120 ${180 + line * 160} 250`} />
          {/each}
        </g>
        <path d="M-20 28L79 41 127 12 190 25 242 64 312 52 340 84 312 109 362 146 348 180 288 198 257 247H-20ZM1200-15H1020L979 33 1007 66 972 101 1013 133 968 170 976 240H1200Z" fill="var(--cover-mid)" opacity="0.25" />
        <g fill="none" stroke="var(--cover-color)" stroke-width="0.8" opacity="0.32">
          <path d="M-20 41L79 54 127 25 190 38 242 77 312 65 327 84 299 109 349 146 335 180 275 198 244 247M1200-2H1033L992 33 1020 66 985 101 1026 133 981 170 989 240" />
          <path d="M-20 54L79 67 127 38 190 51 242 90 307 78M284 111L336 146 322 180 262 198 231 247" />
        </g>
        <path d="M286 239C384 133 471 78 660 120S935 195 1112 81" fill="none" stroke="var(--cover-color)" stroke-width="18" opacity="0.07" />
        <circle cx="660" cy="120" r="92" fill="var(--cover-deep)" opacity="0.09" transform="translate(5 4)" />
        <circle cx="660" cy="120" r="86" fill={`url(#${id}-sphere)`} />
        <g clip-path={`url(#${id}-globe)`}>
          <g fill="var(--cover-highlight)" opacity="0.75">
            <path d="M575 65L603 48 630 57 647 78 628 89 635 102 613 122 594 105 580 103Z" />
            <path d="M612 122L635 131 646 151 634 167 631 185 616 203 608 171 598 151Z" />
            <path d="M665 40L694 52 705 67 729 66 752 93 732 111 719 103 704 117 689 97 671 89 652 71Z" />
            <path d="M672 110L695 113 709 137 696 162 680 175 664 151 660 127Z" />
            <path d="M717 168L737 161 751 179 736 193 718 185Z" />
          </g>
          <g fill="none" stroke="var(--cover-highlight)" stroke-width="0.7" opacity="0.28">
            <ellipse cx="660" cy="120" rx="33" ry="86" /><ellipse cx="660" cy="120" rx="66" ry="86" />
            <ellipse cx="660" cy="120" rx="86" ry="29" /><ellipse cx="660" cy="120" rx="86" ry="59" />
            <path d="M574 120H746M660 34V206" />
          </g>
        </g>
        <path d="M286 239C384 133 471 78 660 120S935 195 1112 81" fill="none" stroke="var(--cover-highlight)" stroke-width="1.4" />
        <path d="M331 205C468 8 764-12 913 129" fill="none" stroke="var(--cover-deep)" stroke-width="0.8" stroke-dasharray="3 5" opacity="0.4" />
        <g fill="var(--cover-highlight)" stroke="var(--cover-color)" stroke-width="1">
          <circle cx="331" cy="205" r="3" /><circle cx="660" cy="120" r="3" /><circle cx="913" cy="129" r="3" />
        </g>
        </g>
      {:else if pattern === "studio"}
        <!-- A continuous design table, from construction drawing through print and pigment. -->
        <path d="M155-30L503-7 490 271 134 258Z" fill="var(--cover-deep)" opacity="0.07" transform="translate(5 5)" />
        <path d="M155-30L503-7 490 271 134 258Z" fill={`url(#${id}-paper)`} opacity="0.85" />
        <g fill="none" stroke="var(--cover-color)" stroke-width="0.8" opacity="0.45">
          <circle cx="354" cy="122" r="84" /><circle cx="354" cy="122" r="59" />
          <path d="M230 122H489M354-12V252M269 37L439 207M269 207L439 37M271 39H437V205H271Z" />
          {#each [0, 60, 120, 180, 240, 300] as angle}
            <ellipse cx="354" cy="91" rx="27" ry="53" transform={`rotate(${angle} 354 122)`} />
          {/each}
        </g>
        <path d="M488-26L881 9 860 270 466 239Z" fill="var(--cover-deep)" opacity="0.12" transform="translate(7 5)" />
        <path d="M488-26L881 9 860 270 466 239Z" fill={`url(#${id}-paper)`} />
        <path d="M514-12L869 18 851 259 491 232Z" fill="var(--cover-mid)" opacity="0.18" />
        <path d="M493 187C586 111 686 72 860 64L855 106C691 107 601 149 487 226Z" fill="var(--cover-color)" opacity="0.3" />
        <g transform="translate(680 122)">
          {#each [0, 60, 120, 180, 240, 300] as angle}
            <path d="M0 0C-65-1-87-55-43-84C-5-102 28-38 0 0Z" transform={`rotate(${angle})`} fill={`url(#${id}-silk)`} stroke="var(--cover-highlight)" stroke-width="0.65" />
          {/each}
          <circle r="18" fill="var(--cover-deep)" />
          <circle r="8" fill="var(--cover-color)" />
        </g>
        <path d="M515 240V214H541M830 18V43H805" fill="none" stroke="var(--cover-deep)" stroke-width="0.8" opacity="0.4" />
        <path d="M-40 203C181 90 323 234 525 154S907 74 1260 167" fill="none" stroke="var(--cover-color)" stroke-width="12" opacity="0.12" />
        <path d="M-40 207C181 94 323 238 525 158S907 78 1260 171" fill="none" stroke="var(--cover-highlight)" stroke-width="0.9" opacity="0.6" />
        <g transform="translate(952 99) rotate(12)">
          <path d="M-44-76H69V78H-44Z" fill={`url(#${id}-paper)`} />
          {#each [0, 1, 2, 3] as swatch}
            <path d="M-29 0Q10-8 55-1L52 21Q10 15-31 21Z" transform={`translate(0 ${-59 + swatch * 34})`} fill={swatch === 0 ? "var(--cover-light)" : swatch === 1 ? "var(--cover-mid)" : swatch === 2 ? "var(--cover-color)" : "var(--cover-deep)"} />
          {/each}
        </g>
        <g transform="rotate(32 1073 155)">
          <path d="M1069 77H1076L1075 249H1071Z" fill="var(--cover-deep)" opacity="0.7" />
          <path d="M1067 57H1078V80H1067Z" fill="var(--cover-mid)" />
          <path d="M1067 57C1061 43 1067 29 1073 17C1072 36 1085 40 1078 57Z" fill={`url(#${id}-night)`} />
          <path d="M1070 59V77" stroke="var(--cover-highlight)" stroke-width="1" />
        </g>
        <g fill="var(--cover-color)" opacity="0.2">
          <circle cx="1090" cy="81" r="9" /><circle cx="1112" cy="102" r="3" /><circle cx="1105" cy="67" r="2" /><circle cx="1131" cy="86" r="4" />
        </g>
      {/if}
    </svg>
  {/if}
</div>

<style>
  .abstract-scene,
  .nature-scene {
    position: absolute;
    width: 100%;
    min-width: 30rem;
    height: 100%;
    left: 50%;
    top: 0;
    transform: translateX(-50%);
  }

  /* Keep subjects proportionate and fully visible vertically; crop only the sides on narrow views. */
  .cover-scene {
    position: absolute;
    height: 100%;
    width: auto;
    aspect-ratio: 5 / 1;
    max-width: none;
    left: 50%;
    top: 0;
    transform: translateX(-50%);
  }
</style>
