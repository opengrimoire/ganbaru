import type { Translate } from "$lib/i18n/translator.svelte";

/** Labels of a recovery offer whose root row lives in one table. */
export interface SyncRecoveryPresentation {
  /** Title of a row whose title is empty. */
  untitled: (t: Translate) => string;
  /** Confirmation after the row was restored. */
  restored: (t: Translate) => string;
  /** Error after the row could not be restored. */
  restoreFailed: (t: Translate) => string;
}

/** How the sync settings present one replicated table. Missing parts use neutral sync copy. */
export interface SyncTablePresentation {
  /** Line for rows of the table with open conflicts, pointing to where they are resolved. */
  conflicts?: (t: Translate, count: number) => string;
  recovery?: SyncRecoveryPresentation;
}

/** A domain whose tables sync, as the sync settings present it. */
export interface SyncDomainPresentation {
  /** Feature name listed in the sync description. */
  label: (t: Translate) => string;
  /** Presentation of each table the domain owns, by replicated table name. */
  tables: Readonly<Record<string, SyncTablePresentation>>;
  /** Tells the open views of the domain that rows of its tables changed outside them. */
  changed: () => void;
}

interface RegisteredTable {
  domain: SyncDomainPresentation;
  presentation: SyncTablePresentation;
}

/** Sync presentation of every registered domain, keyed by replicated table name. */
export class SyncPresentationRegistry {
  private readonly domains: SyncDomainPresentation[] = [];
  private readonly tables = new Map<string, RegisteredTable>();

  /**
   * Adds a domain and its tables.
   *
   * @throws When another domain already registered one of its tables.
   */
  register(domain: SyncDomainPresentation): this {
    const names = Object.keys(domain.tables);
    const taken = names.find((name) => this.tables.has(name));
    if (taken !== undefined) throw new Error(`sync presentation for table ${taken} is already registered`);
    this.domains.push(domain);
    for (const name of names) {
      const presentation = domain.tables[name];
      if (presentation) this.tables.set(name, { domain, presentation });
    }
    return this;
  }

  /** Feature names of the registered domains, in registration order. */
  labels(t: Translate): string[] {
    return this.domains.map((domain) => domain.label(t));
  }

  /** Describes rows of `table` with open conflicts. */
  conflictLine(t: Translate, table: string, count: number): string {
    const conflicts = this.tables.get(table)?.presentation.conflicts;
    return conflicts ? conflicts(t, count) : t("sync.conflicts", count);
  }

  /** Title of a recovery offer rooted in `table` whose title is empty. */
  recoveryUntitled(t: Translate, table: string): string {
    const recovery = this.recovery(table);
    return recovery ? recovery.untitled(t) : t("sync.recovery.untitled");
  }

  /** Confirmation after an offer rooted in `table` was restored. */
  recoveryRestored(t: Translate, table: string): string {
    const recovery = this.recovery(table);
    return recovery ? recovery.restored(t) : t("sync.recovery.restored");
  }

  /** Error after an offer rooted in `table` could not be restored. */
  recoveryRestoreFailed(t: Translate, table: string): string {
    const recovery = this.recovery(table);
    return recovery ? recovery.restoreFailed(t) : t("sync.recovery.failed");
  }

  /** Changed notices of the domains owning `tables`, once per domain. Unknown tables have none. */
  changedNotices(tables: readonly string[]): (() => void)[] {
    const notices = new Set<() => void>();
    for (const table of tables) {
      const registered = this.tables.get(table);
      if (registered) notices.add(registered.domain.changed);
    }
    return [...notices];
  }

  /** Runs the changed notice of every domain owning one of `tables`, once each. */
  notifyChanged(tables: readonly string[]): void {
    for (const notice of this.changedNotices(tables)) notice();
  }

  private recovery(table: string): SyncRecoveryPresentation | undefined {
    return this.tables.get(table)?.presentation.recovery;
  }
}
