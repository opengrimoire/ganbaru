interface ConfirmOptions {
  title?: string;
  yesLabel?: string;
  noLabel?: string;
  extraConfirmShortcut?: (e: KeyboardEvent) => boolean;
}

interface ConfirmationControllerOptions {
  defaultYesLabel: () => string;
  defaultNoLabel: () => string;
}

export function createCalendarViewConfirmationController({
  defaultYesLabel,
  defaultNoLabel,
}: ConfirmationControllerOptions) {
  let action: (() => Promise<void>) | null = $state(null);
  let title: string | undefined = $state(undefined);
  let message = $state("");
  let yesLabel = $state(defaultYesLabel());
  let noLabel = $state(defaultNoLabel());
  let extraShortcut: ((e: KeyboardEvent) => boolean) | undefined = $state(undefined);

  function reset(): void {
    action = null;
    title = undefined;
    message = "";
    extraShortcut = undefined;
  }

  function requestConfirm(
    nextMessage: string,
    nextAction: () => Promise<void>,
    options?: ConfirmOptions,
  ): void {
    title = options?.title;
    message = nextMessage;
    action = nextAction;
    yesLabel = options?.yesLabel ?? defaultYesLabel();
    noLabel = options?.noLabel ?? defaultNoLabel();
    extraShortcut = options?.extraConfirmShortcut;
  }

  async function confirmYes(): Promise<void> {
    const currentAction = action;
    reset();
    if (currentAction) await currentAction();
  }

  function confirmNo(): void {
    reset();
  }

  return {
    get action() {
      return action;
    },
    get title() {
      return title;
    },
    get message() {
      return message;
    },
    get yesLabel() {
      return yesLabel;
    },
    get noLabel() {
      return noLabel;
    },
    get extraShortcut() {
      return extraShortcut;
    },
    requestConfirm,
    confirmYes,
    confirmNo,
  };
}
