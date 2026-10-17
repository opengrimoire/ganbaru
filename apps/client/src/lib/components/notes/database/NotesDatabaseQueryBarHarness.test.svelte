<script lang="ts">
  import { untrack } from "svelte";
  import type { NotesDatabaseQueryProperty } from "$lib/notes/database/query-controls";
  import type { NotesDatabaseTableFilter } from "$lib/notes/types";
  import NotesDatabaseQueryBar from "./NotesDatabaseQueryBar.svelte";

  let { properties, initialFilters }: {
    properties: NotesDatabaseQueryProperty[];
    initialFilters: NotesDatabaseTableFilter[];
  } = $props();
  let filters = $state(untrack(() => initialFilters));
</script>

<NotesDatabaseQueryBar {properties} {filters} sorts={[]} onFiltersChange={(next) => { filters = next; }} onSortsChange={() => {}} />
