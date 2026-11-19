import { Collapsible, CollapsiblePanel, CollapsibleTrigger } from "@ui-sdk/react/collapsible";
import { ChevronRight } from "lucide-react";
import type { ReactNode } from "react";
import { Icon } from "../Icon";
import { Settings } from "../Settings";

export function FoldSection({
  label,
  open = false,
  children,
}: {
  label: string;
  open?: boolean;
  children: ReactNode;
}) {
  return (
    <Collapsible defaultOpen={open} className="border-t border-line">
      <CollapsibleTrigger className="group legend flex h-7 w-full cursor-pointer items-center gap-1 px-2 hover:text-ink">
        <span className="flex transition-transform group-data-open:rotate-90">
          <Icon glyph={ChevronRight} size={12} />
        </span>
        {label}
      </CollapsibleTrigger>
      <CollapsiblePanel keepMounted className="px-2 pb-2">
        {children}
      </CollapsiblePanel>
    </Collapsible>
  );
}

export function SettingsFold({
  label,
  open = false,
  children,
}: {
  label: string;
  open?: boolean;
  children: ReactNode;
}) {
  return (
    <FoldSection label={label} open={open}>
      <Settings>{children}</Settings>
    </FoldSection>
  );
}
