<script lang="ts">
  import type { NotesCoverDesign } from "$lib/notes/contracts/assets";

  let { pattern }: {
    pattern: Extract<NotesCoverDesign, "study" | "mathematics" | "programming" | "finance">;
  } = $props();
  const id = $props.id();
</script>

<!-- The background spans the page; the foreground keeps its drawing proportions. -->
<svg class="absolute inset-0 size-full" viewBox="0 0 1200 240" preserveAspectRatio="none" aria-hidden="true">
  <defs>
    <pattern id={`${id}-ruling`} width="40" height="20" patternUnits="userSpaceOnUse">
      <path d={pattern === "study" ? "M0 20H40" : "M40 0H0V20"} fill="none" stroke={pattern === "mathematics" ? "var(--cover-highlight)" : "var(--cover-color)"} stroke-width="0.6" />
    </pattern>
  </defs>
  <path d="M0 0H1200V240H0Z" fill={`url(#${id}-ruling)`} opacity={pattern === "mathematics" ? 0.1 : 0.13} />
  {#if pattern === "study"}
    <path d="M-80-20H330L234 264H-80ZM933-20H1280V260H1002Z" fill="var(--cover-highlight)" opacity="0.6" />
    <path d="M71 0V240M1081 0V240" stroke="var(--cover-color)" stroke-width="1" opacity="0.22" />
    <g fill="none" stroke="var(--cover-color)" stroke-width="1.5" opacity="0.2">
      <path d="M99 52H202M99 62H247M99 72H218M99 82H233M107 128H197M107 138H232M107 148H208" />
      <path d="M956 168H1062M939 178H1062M970 188H1062M947 198H1062" />
      <path d="M212 171Q315 233 408 203M954 91Q1042 22 1148 62M1135 53L1148 62 1131 68" stroke-width="0.8" />
    </g>
    <path d="M96 80H230M944 178H1064" stroke="var(--cover-color)" stroke-width="10" opacity="0.12" />
  {:else if pattern === "mathematics"}
    <g fill="none" stroke="var(--cover-highlight)" stroke-width="0.7" opacity="0.22">
      <path d="M0 157H1200M81 0V240M1123 0V240" />
      <path d="M-60 211C126 218 125 26 298 39S524 273 706 163 1004 44 1250 86" />
      <path d="M-60 230C126 237 125 45 298 58S524 292 706 182 1004 63 1250 105" stroke-dasharray="2 5" />
      <path d="M67 39V123H179ZM999 191V47L1163 191Z" />
    </g>
    <g fill="var(--cover-highlight)" opacity="0.27" font-family="serif" font-style="italic" font-size="15">
      <text x="132" y="202">a² + b² = c²</text><text x="967" y="30">∫ f(x) dx</text>
    </g>
  {:else if pattern === "programming"}
    <g fill="none" stroke="var(--cover-color)" stroke-width="1" opacity="0.22">
      <path d="M0 57H178L213 92H379M0 75H161L195 109H371M0 204H166L209 161H339M1200 49H1053L1004 98H877M1200 192H1059L1023 156H878M1200 213H1036L1000 177H910" />
      <path d="M102 0V29L152 79M1091 0V24L1053 62M79 240V218L124 173M1135 240V213" />
      {#each [[178, 57], [166, 204], [1053, 49], [1059, 192], [1135, 213]] as port}
        <circle cx={port[0]} cy={port[1]} r="3" />
      {/each}
    </g>
    <g fill="var(--cover-color)" opacity="0.18" font-family="monospace" font-size="30">
      <text x="76" y="145">{'{ }'}</text><text x="1050" y="120">{'</>'}</text>
    </g>
  {:else}
    <path d="M0 29H1200M0 210H1200M0 214H1200" stroke="var(--cover-color)" stroke-width="0.8" opacity="0.3" />
    <g fill="none" stroke="var(--cover-color)" stroke-width="1" opacity="0.22">
      <path d="M95 0V240M243 0V240M993 0V240M1105 0V240" />
      <path d="M0 184L92 166 159 178 253 121 319 139 403 101 479 126 575 80" />
      <path d="M832 153L916 139 993 163 1081 108 1200 70" />
    </g>
    <g fill="var(--cover-color)" opacity="0.24">
      {#each [0, 1, 2, 3, 4, 5] as row}
        <path d={`M${125 + row % 3 * 9} ${49 + row * 23}h72v2h-72zM${1020 + row % 2 * 12} ${49 + row * 23}h54v2h-54z`} />
      {/each}
    </g>
  {/if}
</svg>

<svg class="knowledge-scene" viewBox="0 0 1200 240" aria-hidden="true">
  <defs>
    <linearGradient id={`${id}-paper`} x1="0" y1="0" x2="1" y2="0">
      <stop stop-color="var(--cover-highlight)" /><stop offset="0.46" stop-color="var(--cover-light)" />
      <stop offset="0.51" stop-color="var(--cover-mid)" /><stop offset="0.55" stop-color="var(--cover-highlight)" />
      <stop offset="1" stop-color="var(--cover-light)" />
    </linearGradient>
    <linearGradient id={`${id}-screen`} x1="0" y1="0" x2="1" y2="1">
      <stop stop-color="var(--cover-deep)" /><stop offset="1" stop-color="var(--cover-color)" />
    </linearGradient>
  </defs>
  {#if pattern === "study"}
    <!-- Page edges, annotations, and a concept map overlap within one study spread. -->
    <path d="M253 11L899 33 885 245 231 223Z" fill="var(--cover-mid)" opacity="0.26" />
    <path d="M294 26Q427 4 575 30Q722 8 872 29L881 218Q722 197 583 221Q442 201 285 218Z" fill="var(--cover-deep)" opacity="0.12" transform="translate(7 5)" />
    <path d="M294 26Q427 4 575 30Q722 8 872 29L881 218Q722 197 583 221Q442 201 285 218Z" fill={`url(#${id}-paper)`} />
    <path d="M575 30L583 221M291 212Q443 195 583 215Q723 191 875 212" fill="none" stroke="var(--cover-color)" stroke-width="0.8" opacity="0.35" />
    <path d="M329 56H462M329 64H412" stroke="var(--cover-deep)" stroke-width="3" opacity="0.7" />
    <g fill="none" stroke="var(--cover-deep)" stroke-width="1.3" opacity="0.47">
      {#each [0, 1, 2, 3, 4] as line}
        <path d={`M330 ${85 + line * 10}h${174 - line % 3 * 19}M625 ${151 + line * 10}h${195 - line % 3 * 23}`} />
      {/each}
      <path d="M330 153H509M330 163H487M330 173H502M330 183H458" />
    </g>
    <path d="M329 104H484M625 170H795M329 172H503" stroke="var(--cover-color)" stroke-width="8" opacity="0.25" />
    <path d="M322 90H315V124H322M619 159H612V184H619" fill="none" stroke="var(--cover-color)" stroke-width="1.2" />
    <g fill="var(--cover-light)" stroke="var(--cover-color)" stroke-width="1">
      <rect x="680" y="51" width="72" height="23" rx="3" />
      <rect x="623" y="105" width="56" height="22" rx="3" /><rect x="699" y="105" width="56" height="22" rx="3" /><rect x="775" y="105" width="56" height="22" rx="3" />
      <path d="M716 74V89H651V105M716 89H803V105M727 89V105" fill="none" />
    </g>
    <path d="M693 62H739M634 116H666M710 116H744M786 116H820" stroke="var(--cover-color)" stroke-width="2" />
    <g transform="rotate(9 929 89)">
      <path d="M860 34H1000V144H860Z" fill="var(--cover-deep)" opacity="0.09" transform="translate(4 4)" />
      <path d="M860 34H1000V144H860Z" fill="var(--cover-mid)" /><path d="M876 56H970M876 66H980M876 76H951M876 99H968M876 109H982" stroke="var(--cover-highlight)" stroke-width="2" />
      <path d="M921 31H952V41H921Z" fill="var(--cover-highlight)" opacity="0.6" />
    </g>
    <path d="M917 149C935 182 892 202 839 183M849 180L839 183 845 193" fill="none" stroke="var(--cover-color)" stroke-width="1.2" />
    <path d="M163 191Q225 143 305 117M294 117H305L301 128" fill="none" stroke="var(--cover-color)" stroke-width="1" stroke-dasharray="3 4" opacity="0.6" />
    <path d="M262 20H273L263 111 257 124 253 109Z" fill="var(--cover-color)" /><path d="M257 124L255 116 262 117Z" fill="var(--cover-deep)" />
  {:else if pattern === "mathematics"}
    <!-- A circle, tangent, and area construction occupy the same coordinate surface. -->
    <g fill="none" stroke="var(--cover-highlight)" stroke-width="0.8" opacity="0.25">
      <path d="M211 195H974M414 18V223M211 47H974M211 84H974M211 121H974M211 158H974" />
      <path d="M266 26V214M340 26V214M488 26V214M562 26V214M636 26V214M710 26V214M784 26V214M858 26V214M932 26V214" />
    </g>
    <path d="M488 195V158C534 88 591 53 636 47C696 38 746 70 784 90V195Z" fill="var(--cover-highlight)" opacity="0.07" />
    <g stroke="var(--cover-highlight)" stroke-width="0.65" opacity="0.2">
      <path d="M505 195V136M522 195V114M539 195V96M556 195V82M573 195V69M590 195V59M607 195V53M624 195V48M641 195V46M658 195V47M675 195V50M692 195V55M709 195V62M726 195V71M743 195V80M760 195V87" />
    </g>
    <path d="M232 229C352 275 424 234 488 158S592 49 636 47C747 26 793 135 961 147" fill="none" stroke="var(--cover-highlight)" stroke-width="2" />
    <path d="M402 195H965M955 190L965 195 955 200M414 213V22M409 32L414 22 419 32" fill="none" stroke="var(--cover-highlight)" stroke-width="1.1" opacity="0.85" />
    <circle cx="710" cy="121" r="74" fill="none" stroke="var(--cover-highlight)" stroke-width="1.1" opacity="0.75" />
    <path d="M710 121L762 69 762 121ZM673-20L911 218" fill="none" stroke="var(--cover-highlight)" stroke-width="0.9" opacity="0.7" />
    <path d="M750 121V109H762M730 121A20 20 0 0 0 724 107" fill="none" stroke="var(--cover-highlight)" stroke-width="0.8" />
    <g fill="var(--cover-highlight)">
      <circle cx="762" cy="69" r="3" /><circle cx="710" cy="121" r="2" /><circle cx="636" cy="47" r="2.5" />
    </g>
    <!-- Mathematical notation is language-independent decorative content. -->
    <g fill="var(--cover-highlight)" font-family="serif" font-style="italic" opacity="0.8">
      <text x="429" y="30" font-size="14">y</text><text x="968" y="212" font-size="14">x</text>
      <text x="735" y="91" font-size="12">r</text><text x="572" y="179" font-size="19">∫ f(x) dx</text>
      <text x="220" y="66" font-size="14">(x−h)² + (y−k)² = r²</text><text x="850" y="61" font-size="16">f(x)</text>
    </g>
    <path d="M260 76Q336 81 384 74M844 70H889" stroke="var(--cover-highlight)" stroke-width="0.7" opacity="0.4" />
  {:else if pattern === "programming"}
    <!-- Code indentation feeds into connected execution paths, with traces continuing beyond the screen. -->
    <path d="M210 18H653V222H210Z" fill="var(--cover-deep)" opacity="0.12" transform="translate(6 5)" />
    <rect x="210" y="18" width="443" height="204" rx="7" fill={`url(#${id}-screen)`} />
    <path d="M210 47H653" stroke="var(--cover-highlight)" stroke-width="0.7" opacity="0.2" />
    <g fill="var(--cover-highlight)" opacity="0.5">
      <circle cx="227" cy="33" r="2" /><circle cx="236" cy="33" r="2" /><circle cx="245" cy="33" r="2" />
    </g>
    <g fill="none" stroke="var(--cover-highlight)" stroke-width="0.7" opacity="0.16">
      <path d="M249 60V206M273 82V183M297 105V159M321 126V151" />
    </g>
    <g stroke="var(--cover-highlight)" stroke-width="3" opacity="0.8">
      <path d="M272 69H322M342 69H395M293 91H340M358 91H429M314 113H367M384 113H485M337 135H398M413 135H449M314 157H343M293 179H369M272 201H303" />
    </g>
    <g stroke="var(--cover-mid)" stroke-width="3">
      <path d="M412 69H467M446 91H495M502 113H568M465 135H535M361 157H438M387 179H463" />
    </g>
    <!-- Brackets and operators are syntax, not application copy. -->
    <g fill="var(--cover-highlight)" font-family="monospace" font-size="15" opacity="0.8">
      <text x="485" y="73">{'{'}</text><text x="512" y="95">{'['}</text><text x="548" y="139">{'()'}</text>
      <text x="450" y="161">{']'}</text><text x="478" y="183">{'}'}</text><text x="276" y="36" font-size="12">{'</>'}</text>
    </g>
    <g fill="none" stroke="var(--cover-color)" stroke-width="1.5">
      <path d="M582 113H710V67H785M710 113V164H785M834 67H938V116M834 164H938V126M953 121H1034V206H697V182H653" />
      <path d="M775 63L785 67 775 71M775 160L785 164 775 168M930 106L938 116 946 106M663 178L653 182 663 186" />
    </g>
    <g fill="var(--cover-highlight)" stroke="var(--cover-color)" stroke-width="1.2">
      <rect x="785" y="50" width="50" height="34" rx="4" /><rect x="785" y="147" width="50" height="34" rx="4" />
      <path d="M938 103L956 121 938 139 920 121Z" />
      <circle cx="710" cy="113" r="4" />
    </g>
    <g fill="var(--cover-color)" font-family="monospace" font-size="14">
      <text x="798" y="73">{'{}'}</text><text x="798" y="169">{'[]'}</text>
    </g>
    <path d="M810 29V18H1047V232H810V210" fill="none" stroke="var(--cover-color)" stroke-width="0.7" opacity="0.25" stroke-dasharray="3 5" />
  {:else}
    <!-- A ledger's rows become chart guides, so accounting and trends share one surface. -->
    <path d="M178-16L994 9 985 255 166 228Z" fill="var(--cover-deep)" opacity="0.07" transform="translate(5 5)" />
    <path d="M178-16L994 9 985 255 166 228Z" fill="var(--cover-highlight)" opacity="0.75" />
    <g fill="none" stroke="var(--cover-color)" stroke-width="0.8" opacity="0.3">
      <path d="M188 43H973M188 198H973M188 202H973M307 22V220M410 22V220M487 22V220M943 22V220" />
      {#each [0, 1, 2, 3, 4] as row}
        <path d={`M188 ${65 + row * 27}H973`} />
      {/each}
    </g>
    <g stroke="var(--cover-deep)" opacity="0.6" stroke-width="2">
      {#each [0, 1, 2, 3, 4] as row}
        <path d={`M${219 + row % 2 * 14} ${78 + row * 27}H285M${346 + row % 3 * 9} ${78 + row * 27}H388M${446 + row % 2 * 9} ${78 + row * 27}H471`} />
      {/each}
    </g>
    <g fill="var(--cover-color)" font-family="serif" font-size="19">
      <text x="249" y="34">Σ</text><text x="356" y="35">+</text><text x="443" y="35">−</text>
    </g>
    <g fill="var(--cover-color)" opacity="0.3">
      <path d="M544 142H578V198H544ZM604 111H638V198H604ZM664 127H698V198H664ZM724 85H758V198H724ZM784 99H818V198H784ZM844 64H878V198H844Z" />
    </g>
    <g fill="var(--cover-color)" opacity="0.6">
      <path d="M544 142H578V157H544ZM604 111H638V137H604ZM664 127H698V147H664ZM724 85H758V118H724ZM784 99H818V126H784ZM844 64H878V101H844Z" />
    </g>
    <path d="M505 168C548 169 581 106 621 111S658 148 681 139S715 70 741 90S775 113 801 105S842 53 870 60" fill="none" stroke="var(--cover-deep)" stroke-width="1.8" />
    <g fill="var(--cover-highlight)" stroke="var(--cover-deep)" stroke-width="1.2">
      <circle cx="621" cy="111" r="3" /><circle cx="681" cy="139" r="3" /><circle cx="741" cy="90" r="3" /><circle cx="801" cy="105" r="3" /><circle cx="870" cy="60" r="3" />
    </g>
    <path d="M192 204H473M929 55V181M925 55H933M925 181H933" stroke="var(--cover-color)" stroke-width="0.8" />
    <path d="M706 30H823M718 36H836" stroke="var(--cover-color)" stroke-width="7" opacity="0.1" />
  {/if}
</svg>

<style>
  .knowledge-scene {
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
