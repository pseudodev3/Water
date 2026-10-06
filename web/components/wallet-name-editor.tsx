"use client";

import { FormEvent, useEffect, useId, useRef, useState } from "react";
import { chainLabel, WalletCandidate } from "@/lib/wallets";

export function WalletNameEditor({
  candidate,
  initialName,
  onSave,
  onClose,
}: {
  candidate: WalletCandidate;
  initialName: string;
  onSave: (name: string) => string | null;
  onClose: () => void;
}) {
  const [name, setName] = useState(initialName);
  const [error, setError] = useState("");
  const dialog = useRef<HTMLDialogElement>(null);
  const id = useId();

  useEffect(() => {
    const element = dialog.current;
    const opener = document.activeElement;
    element?.showModal();
    return () => {
      if (element?.open) element.close();
      if (opener instanceof HTMLElement && opener.isConnected)
        opener.focus({ preventScroll: true });
    };
  }, []);

  function save(event: FormEvent) {
    event.preventDefault();
    const failure = onSave(name.trim());
    if (failure) setError(failure);
    else dialog.current?.close();
  }

  return (
    <dialog
      ref={dialog}
      className="wallet-name-dialog"
      aria-labelledby={`${id}-title`}
      aria-describedby={`${id}-help`}
      onClose={onClose}
    >
      <form onSubmit={save}>
        <h2 id={`${id}-title`}>Rename wallet</h2>
        <p className="wallet-name-address">
          {chainLabel(candidate.chain)} · {candidate.wallet}
        </p>
        <label htmlFor={`${id}-name`}>Wallet name</label>
        <input
          id={`${id}-name`}
          autoFocus
          autoComplete="off"
          maxLength={64}
          placeholder="Give it a name"
          value={name}
          onChange={(event) => {
            setName(event.target.value);
            setError("");
          }}
        />
        <p id={`${id}-help`} className="wallet-name-help">
          Saved in this browser, like your follows. Leave blank to use its
          address.
        </p>
        {error && (
          <p className="wallet-name-error" role="alert">
            {error}
          </p>
        )}
        <div className="wallet-name-actions">
          <button
            className="wallet-inline-button"
            type="button"
            onClick={() => dialog.current?.close()}
          >
            Cancel
          </button>
          <button className="wallet-inline-button" type="submit">
            Save name
          </button>
        </div>
      </form>
    </dialog>
  );
}
