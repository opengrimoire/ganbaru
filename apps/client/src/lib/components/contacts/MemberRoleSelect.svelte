<script lang="ts">
  import Select from "$lib/components/ui/Select.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { INVITABLE_MEMBER_ROLES, MEMBER_ROLES, type MemberRole } from "./model";

  /**
   * Quiet role control shown next to a member. It renders the final look of a role change and stays unavailable
   * until a caller provides `onChange`.
   */
  let {
    value = "member",
    ariaLabel,
    roles = INVITABLE_MEMBER_ROLES,
    onChange,
  }: {
    value?: MemberRole;
    ariaLabel: string;
    roles?: readonly MemberRole[];
    onChange?: (role: MemberRole) => void;
  } = $props();

  const { t } = getLocalization();
  const options = $derived(roles.map((role) => ({
    value: role,
    label: t(`contacts.role.${role}`),
    summary: t(`contacts.role.summary.${role}`),
  })));

  function changeRole(next: string): void {
    const role = MEMBER_ROLES.find((entry) => entry === next);
    if (role) onChange?.(role);
  }
</script>

<Select inline appearance="quiet" contentAlign="end" {value} {options} {ariaLabel} unavailable={!onChange} onChange={changeRole} />
