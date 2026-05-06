import { t } from "@front-shared/i18n";
import { state } from "@front-shared/state";
import {
  disableGateway,
  enableGateway,
  getGatewayStatus,
  recoverGateway,
  updateGatewaySettings,
} from "@front-shared/tauri";
import { elements, renderGateway, showToast } from "@front-shared/render";
import type { GatewayStatus, GatewayUpdatePayload } from "@front-shared/types";

function applyStatus(status: GatewayStatus): void {
  state.gateway = status;
  renderGateway(status);
}

function setBusy(busy: boolean): void {
  state.gatewayBusy = busy;
  elements.gatewayToggleInput.disabled = busy;
  elements.gatewayApplyButton.disabled = busy;
  elements.gatewayRecoverButton.disabled = busy;
}

export async function loadGatewayStatus(options?: { silent?: boolean }): Promise<boolean> {
  try {
    const status = await getGatewayStatus();
    applyStatus(status);
    return true;
  } catch (error) {
    if (!options?.silent) {
      showToast(error instanceof Error ? error.message : "Failed to load gateway status", true);
    }
    return false;
  }
}

export async function handleToggleGateway(nextEnabled: boolean): Promise<void> {
  if (state.gatewayBusy) {
    return;
  }
  setBusy(true);
  try {
    const status = nextEnabled ? await enableGateway() : await disableGateway();
    applyStatus(status);
    if (nextEnabled) {
      showToast(t(state.locale, "gatewayEnabledToast", { endpoint: status.endpoint }));
    } else {
      showToast(t(state.locale, "gatewayDisabledToast"));
    }
  } catch (error) {
    showToast(
      error instanceof Error
        ? error.message
        : t(state.locale, nextEnabled ? "gatewayFailedEnable" : "gatewayFailedDisable"),
      true,
    );
    // Re-sync UI from backend so the toggle reflects truth.
    await loadGatewayStatus();
  } finally {
    setBusy(false);
  }
}

function readControlValues(): GatewayUpdatePayload {
  const portValue = Number.parseInt(elements.gatewayPortInput.value, 10);
  return {
    port: Number.isFinite(portValue) ? portValue : undefined,
    session_affinity: elements.gatewayAffinityInput.checked,
    strategy: elements.gatewayStrategySelect.value,
  };
}

export async function handleApplyGatewaySettings(): Promise<void> {
  if (state.gatewayBusy) {
    return;
  }
  setBusy(true);
  try {
    const status = await updateGatewaySettings(readControlValues());
    applyStatus(status);
    showToast(t(state.locale, "gatewayUpdatedToast"));
  } catch (error) {
    showToast(error instanceof Error ? error.message : t(state.locale, "gatewayFailedUpdate"), true);
    await loadGatewayStatus();
  } finally {
    setBusy(false);
  }
}

export async function handleRecoverGateway(): Promise<void> {
  if (state.gatewayBusy) {
    return;
  }
  setBusy(true);
  try {
    const status = await recoverGateway();
    applyStatus(status);
    showToast(t(state.locale, "gatewayRecoveredToast"));
  } catch (error) {
    showToast(error instanceof Error ? error.message : t(state.locale, "gatewayFailedRecover"), true);
    await loadGatewayStatus();
  } finally {
    setBusy(false);
  }
}
