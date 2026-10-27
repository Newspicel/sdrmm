import {
  type BroadcastStatus,
  ChoiceChip,
  choicesOf,
  limitOf,
  type Mode,
  NumberChip,
  OptionalNumberChip,
  ServiceChip,
  ToggleChip,
} from "./kit";

export function DatvChips({
  params,
  choices,
  broadcast,
  limits,
  onParams,
}: Mode<"datv"> & { broadcast?: BroadcastStatus }) {
  const settings = params.settings;
  const set = (next: typeof settings) => onParams({ type: "datv", settings: next });
  const codeRate = settings.code_rate;
  return (
    <>
      <ChoiceChip
        label="Standard"
        title="DATV standard"
        value={settings.standard}
        options={choicesOf(choices, params.settings, "standard")}
        onChange={(standard) => set({ ...settings, standard })}
      />
      <NumberChip
        label="Rate"
        title="DATV symbol rate"
        value={settings.symbol_rate}
        {...limitOf(limits, "symbol_rate")}
        unit="Bd"
        onCommit={(symbol_rate) => set({ ...settings, symbol_rate })}
      />
      <ServiceChip
        status={broadcast}
        value={settings.program ?? null}
        max={65535}
        onChange={(program) => set({ ...settings, program })}
      />
      {settings.standard === "dvb_s2" ? (
        <>
          <ChoiceChip
            label="Roll-off"
            title="Match the transmitter's filter shape; DVB-S2 signals it in the base band header"
            value={settings.roll_off}
            options={choicesOf(choices, params.settings, "roll_off")}
            onChange={(roll_off) => set({ ...settings, roll_off })}
          />
          <ToggleChip
            label="Superframes"
            title="Receive DVB-S2X Annex E superframes, formats 0 to 7"
            on={settings.superframes}
            onChange={(superframes) => set({ ...settings, superframes })}
          />
          {settings.superframes ? (
            <>
              <NumberChip
                label="Ref code"
                title="Superframe reference scrambling code n_Ref, 0 by default"
                value={settings.superframe_reference}
                {...limitOf(limits, "superframe_reference")}
                onCommit={(superframe_reference) => set({ ...settings, superframe_reference })}
              />
              <NumberChip
                label="Data code"
                title="Superframe payload scrambling code n_Pay, 0 by default"
                value={settings.superframe_payload}
                {...limitOf(limits, "superframe_payload")}
                onCommit={(superframe_payload) => set({ ...settings, superframe_payload })}
              />
              <ToggleChip
                label="Code search"
                title="Find unknown scrambling codes from the signal; needs a clean carrier"
                on={settings.superframe_search}
                onChange={(superframe_search) => set({ ...settings, superframe_search })}
              />
            </>
          ) : null}
          <OptionalNumberChip
            label="Stream"
            title="Choose an input stream identifier on a multistream carrier"
            placeholder="Auto"
            value={settings.input_stream ?? null}
            min={0}
            max={255}
            step={1}
            onCommit={(input_stream) => set({ ...settings, input_stream })}
          />
        </>
      ) : (
        <ChoiceChip
          label="FEC"
          title="DVB-S code rate"
          value={codeRate}
          options={choicesOf(choices, params.settings, "code_rate")}
          quiet={codeRate === "auto"}
          onChange={(code_rate) => set({ ...settings, code_rate })}
        />
      )}
    </>
  );
}
