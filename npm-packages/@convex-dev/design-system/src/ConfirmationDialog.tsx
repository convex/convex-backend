import React, { ReactNode, useState } from "react";
import { TextInput } from "@ui/TextInput";
import { Button } from "@ui/Button";
import { Modal } from "@ui/Modal";

export function ConfirmationDialog({
  onClose,
  onBeforeClose,
  onConfirm,
  validationText,
  confirmText,
  dialogTitle,
  dialogBody,
  disableCancel = false,
  disableConfirm = false,
  variant = "danger",
  error,
}: {
  onClose: () => void;
  onBeforeClose?: () => boolean;
  onConfirm: () => Promise<void>;
  disableCancel?: boolean;
  disableConfirm?: boolean;
  confirmText: string;
  validationText?: string;
  dialogTitle: ReactNode;
  dialogBody: ReactNode;
  variant?: "primary" | "danger" | "neutral" | "unstyled";
  error?: ReactNode;
}) {
  const [validation, setValidation] = useState("");
  const [isConfirming, setIsConfirming] = useState(false);

  const handleConfirm = async () => {
    setIsConfirming(true);
    try {
      await onConfirm();
      onClose();
    } catch {
      // Do nothing on error. HTTP errors get handled by the useMutation hook.
    } finally {
      setIsConfirming(false);
    }
  };

  const disabled =
    disableConfirm || isConfirming || validationText
      ? validation.trimStart().trimEnd().toLowerCase() !==
        validationText?.trimStart().trimEnd().toLowerCase()
      : false;

  return (
    <Modal
      title={dialogTitle}
      onClose={onClose}
      onBeforeClose={onBeforeClose}
      size="sm"
    >
      <div className="pb-3">
        {dialogBody}{" "}
        {validationText && (
          <>
            <div className="mt-4 text-sm">
              Type{" "}
              <code className="rounded-sm bg-background-tertiary p-1 text-sm text-content-primary">
                {validationText}
              </code>{" "}
              in the box below to confirm.
            </div>
            <TextInput
              id="validation"
              aria-label={`Enter ${validationText} to continue`}
              labelHidden
              onKeyDown={(e) =>
                e.key === "Enter" && !disabled && handleConfirm()
              }
              autoFocus
              outerClassname="mt-4"
              placeholder={validationText}
              onChange={(event) => setValidation(event.target.value)}
              value={validation}
              error={error}
            />
          </>
        )}
      </div>
      <div className="flex w-full gap-2">
        <div className="grow">&nbsp;</div>
        <Button
          variant="neutral"
          onClick={() => {
            if (!onBeforeClose || onBeforeClose()) {
              onClose();
            }
          }}
          disabled={disableCancel}
        >
          Cancel
        </Button>
        <Button
          data-testid="confirm-button"
          disabled={disabled || isConfirming}
          onClick={handleConfirm}
          loading={isConfirming}
          variant={variant}
        >
          {confirmText}
        </Button>
      </div>
    </Modal>
  );
}
