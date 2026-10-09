import { useState } from "react";
import { Button, Form, Input } from "../components/BaseControls";
import { BTN_QUIET, FIELD, LABEL } from "../components/controls";
import { Popover } from "../components/Popover";
import { PRESENCE_LIMITS } from "../lib/limits";
import {
  authorKey,
  initialOf,
  type Person,
  peerColor,
  savedName,
  saveName,
  usePeople,
} from "../lib/presence";
import { useWorkspaceContext } from "./context";

const STACKED = 3;

export function Peers() {
  const { me, others } = usePeople();
  if (me === null) {
    return null;
  }
  const stacked = others.length === 0 ? [me] : others.slice(0, STACKED);
  const hidden = others.length - stacked.length;
  return (
    <Popover
      label={
        <span className="flex items-center">
          {stacked.map((person) => (
            <Avatar key={`${person.hue}:${person.name}`} person={person} stacked />
          ))}
          {hidden > 0 && (
            <span className="ml-1 font-mono text-[10px] text-ink-dim tabular-nums">+{hidden}</span>
          )}
        </span>
      }
      title={others.length === 0 ? "Only you" : `${others.length + 1} here`}
      triggerClass="flex h-6 items-center rounded-full px-1 hover:bg-panel-3"
      align="end"
      width="w-60"
    >
      {(close) => <PeopleList me={me} others={others} onDone={close} />}
    </Popover>
  );
}

function Avatar({ person, stacked = false }: { person: Person; stacked?: boolean }) {
  return (
    <span
      aria-hidden
      className={`flex size-[18px] shrink-0 items-center justify-center rounded-full text-[9px] font-semibold text-bg ring-2 ring-panel ${
        stacked ? "-ml-1.5 first:ml-0" : ""
      }`}
      style={{ background: peerColor(person) }}
    >
      {initialOf(person.name)}
    </span>
  );
}

function Row({ person, note }: { person: Person; note?: string }) {
  return (
    <li className="flex h-6 items-center gap-2 text-xs">
      <Avatar person={person} />
      <span className="min-w-0 flex-1 truncate">{person.name}</span>
      {person.tabs > 1 && (
        <span className="font-mono text-[10px] text-ink-faint tabular-nums">
          {person.tabs} tabs
        </span>
      )}
      {note !== undefined && <span className="text-[10px] text-ink-faint">{note}</span>}
    </li>
  );
}

function PeopleList({
  me,
  others,
  onDone,
}: {
  me: Person;
  others: readonly Person[];
  onDone: () => void;
}) {
  return (
    <div className="flex flex-col gap-2">
      <span className={LABEL}>Here now</span>
      <ul className="flex max-h-60 flex-col gap-0.5 overflow-y-auto">
        <Row person={me} note="you" />
        {others.map((person) => (
          <Row key={`${person.hue}:${person.name}`} person={person} />
        ))}
      </ul>
      <NameForm current={me.name} onDone={onDone} />
    </div>
  );
}

function NameForm({ current, onDone }: { current: string; onDone: () => void }) {
  const { socket } = useWorkspaceContext();
  const [name, setName] = useState(savedName() || current);
  return (
    <Form
      className="flex gap-1 border-t border-line pt-2"
      onSubmit={(event) => {
        event.preventDefault();
        saveName(name);
        socket.send({ type: "Present", data: { author: authorKey(), name: name.trim() } });
        onDone();
      }}
    >
      <Input
        className={`${FIELD} min-w-0 flex-1`}
        aria-label="Your name"
        placeholder="Your name"
        maxLength={PRESENCE_LIMITS.name_len}
        value={name}
        onChange={(event) => setName(event.target.value)}
      />
      <Button type="submit" className={BTN_QUIET}>
        Save
      </Button>
    </Form>
  );
}
