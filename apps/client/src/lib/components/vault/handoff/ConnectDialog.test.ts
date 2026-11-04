// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { VaultBootstrapOutcome } from "$lib/api/vault-handoff";
import ConnectDialog from "./ConnectDialog.svelte";

const joinVaultThroughServer = vi.hoisted(() => vi.fn<(code: string) => Promise<VaultBootstrapOutcome>>());
const recoverVaultThroughServer = vi.hoisted(() => vi.fn<(code: string, kit: string) => Promise<VaultBootstrapOutcome>>());
vi.mock("$lib/api/vault-handoff", () => ({ joinVaultThroughServer, recoverVaultThroughServer }));

const CODE = "ganbaru-anchor:abc";
const KIT = "AAAA-BBBB-CCCC";
const ACTIVATED: VaultBootstrapOutcome = { transferId: "t", activated: true, inProgress: false };

describe("ConnectDialog", () => {
  let target: HTMLDivElement;
  let component: ReturnType<typeof mount> | null = null;
  const onSubmit = vi.fn();
  const onClose = vi.fn();
  const onServerActivated = vi.fn();

  beforeEach(() => {
    joinVaultThroughServer.mockReset();
    recoverVaultThroughServer.mockReset();
    onSubmit.mockReset();
    onClose.mockReset();
    onServerActivated.mockReset();
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ConnectDialog, {
      target,
      props: { busy: false, enrollmentCode: null, error: null, onSubmit, onEdit: vi.fn(), onClose, onServerActivated },
    });
    flushSync();
  });

  afterEach(async () => {
    if (component) await unmount(component);
    target.remove();
    component = null;
    vi.restoreAllMocks();
  });

  function field(id: string): HTMLInputElement {
    const input = target.querySelector<HTMLInputElement>(`#${id}`);
    if (!input) throw new Error(`missing field ${id}`);
    return input;
  }

  function enter(input: HTMLInputElement, value: string): void {
    input.value = value;
    input.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
  }

  function submitButton(): HTMLButtonElement {
    const button = target.querySelector<HTMLButtonElement>("[data-connect-submit]");
    if (!button) throw new Error("missing submit button");
    return button;
  }

  async function chooseServer(): Promise<void> {
    target.querySelector<HTMLButtonElement>("#connect-tab-server")?.click();
    await tick();
    flushSync();
  }

  async function showKit(): Promise<void> {
    const button = [...target.querySelectorAll<HTMLButtonElement>("button")]
      .find((candidate) => candidate.textContent?.includes("recovery kit"));
    if (!button) throw new Error("missing kit toggle");
    button.click();
    await tick();
    flushSync();
  }

  async function submit(): Promise<void> {
    submitButton().click();
    await tick();
    await vi.waitFor(() => expect(field("server-connect-code").disabled).toBe(false));
  }

  it("hands a pairing code to the panel from the computer tab", () => {
    const input = target.querySelector<HTMLInputElement>('input[type="text"]');
    if (!input) throw new Error("missing pairing field");
    expect(submitButton().disabled).toBe(true);
    enter(input, "  pairing-code \n");
    submitButton().click();
    expect(onSubmit).toHaveBeenCalledWith("pairing-code");
    expect(joinVaultThroughServer).not.toHaveBeenCalled();
  });

  it("joins through the server with only the trimmed setup code", async () => {
    joinVaultThroughServer.mockResolvedValue(ACTIVATED);
    await chooseServer();
    expect(submitButton().disabled).toBe(true);
    enter(field("server-connect-code"), `  ${CODE}\n`);
    await submit();
    expect(joinVaultThroughServer).toHaveBeenCalledWith(CODE);
    expect(recoverVaultThroughServer).not.toHaveBeenCalled();
    expect(onServerActivated).toHaveBeenCalledOnce();
    expect(field("server-connect-code").value).toBe("");
  });

  it("needs the kit once it is shown and recovers with it", async () => {
    recoverVaultThroughServer.mockResolvedValue(ACTIVATED);
    await chooseServer();
    enter(field("server-connect-code"), CODE);
    await showKit();
    expect(submitButton().disabled).toBe(true);
    enter(field("server-connect-kit"), KIT);
    await submit();
    expect(recoverVaultThroughServer).toHaveBeenCalledWith(CODE, KIT);
    expect(joinVaultThroughServer).not.toHaveBeenCalled();
    expect(onServerActivated).toHaveBeenCalledOnce();
  });

  it("forgets the kit after a refusal, keeps the code, and logs nothing", async () => {
    const warn = vi.spyOn(console, "warn");
    const failure = vi.spyOn(console, "error");
    recoverVaultThroughServer.mockRejectedValue("read the recovery kit: invalid length");
    await chooseServer();
    enter(field("server-connect-code"), CODE);
    await showKit();
    enter(field("server-connect-kit"), KIT);
    await submit();
    expect(onServerActivated).not.toHaveBeenCalled();
    expect(field("server-connect-kit").value).toBe("");
    expect(field("server-connect-code").value).toBe(CODE);
    expect(target.textContent).not.toContain(KIT);
    expect(target.querySelector('[role="alert"]')?.textContent).toContain("recovery kit");
    expect(warn).not.toHaveBeenCalled();
    expect(failure).not.toHaveBeenCalled();
  });

  it("explains that the server waits for the anchor and keeps the code for a retry", async () => {
    joinVaultThroughServer.mockRejectedValue("the server holds this enrollment until the vault's anchor is online");
    await chooseServer();
    enter(field("server-connect-code"), CODE);
    await submit();
    expect(onServerActivated).not.toHaveBeenCalled();
    expect(field("server-connect-code").value).toBe(CODE);
    expect(target.querySelector('[role="alert"]')?.textContent).toContain("anchor is online");
  });

  it("does not report activation while another copy is running", async () => {
    joinVaultThroughServer.mockResolvedValue({ transferId: null, activated: false, inProgress: true });
    await chooseServer();
    enter(field("server-connect-code"), CODE);
    await submit();
    expect(onServerActivated).not.toHaveBeenCalled();
    expect(target.querySelector('[role="status"]')).not.toBeNull();
  });

  it("stays open and keeps the tabs still while the server connects", async () => {
    let finish: (outcome: VaultBootstrapOutcome) => void = () => undefined;
    joinVaultThroughServer.mockReturnValue(new Promise((resolve) => { finish = resolve; }));
    await chooseServer();
    enter(field("server-connect-code"), CODE);
    submitButton().click();
    await tick();
    flushSync();
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    target.querySelector<HTMLDivElement>('[role="dialog"]')?.parentElement?.click();
    expect(onClose).not.toHaveBeenCalled();
    expect(target.querySelector<HTMLButtonElement>("#connect-tab-computer")?.disabled).toBe(true);
    finish(ACTIVATED);
    await vi.waitFor(() => expect(onServerActivated).toHaveBeenCalledOnce());
  });
});
