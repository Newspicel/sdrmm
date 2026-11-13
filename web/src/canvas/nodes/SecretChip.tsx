import { TextChip } from "../../components/face/TextChip";
import { keepSecret } from "../../lib/api";
import { toastError } from "../../lib/toasts";
import type { Secret } from "../../lib/types";
import { secretSet } from "./eventOutput";

/** A credential field: what is typed goes to this server's store, the node keeps its id. */
export function SecretChip({
  label,
  title,
  name,
  secret,
  onSet,
}: {
  label: string;
  title: string;
  name?: string;
  secret: Secret | undefined;
  onSet: (next: Secret) => void;
}) {
  const set = secretSet(secret);
  return (
    <TextChip
      label={label}
      name={name}
      title={title}
      value=""
      shown={set ? "set" : "none"}
      quiet={!set}
      secret
      onCommit={(value) => {
        keepSecret(value).then(onSet).catch(toastError);
      }}
    />
  );
}
