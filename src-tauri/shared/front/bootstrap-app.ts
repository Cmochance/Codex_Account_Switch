import { bootstrap } from "@front-shared/actions";

export function bootstrapDesktopShell(
  setupWindowControls: () => void | Promise<void>,
): void {
  void setupWindowControls();
  bootstrap();
}
