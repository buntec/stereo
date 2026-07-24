import "./App.css";
import type { ComponentType, SVGProps } from "react";
import { MoonIcon } from "@radix-ui/react-icons";

import { Switch } from "@astryxdesign/core/Switch";
import { HStack } from "@astryxdesign/core/Stack";
import { Icon } from "@astryxdesign/core/Icon";
import type { AppearanceType } from "./Types.tsx";

// @radix-ui/react-icons components declare `children?: undefined`, which
// doesn't structurally satisfy Astryx's ComponentType<SVGProps<...>> icon slot.
const MoonSvgIcon = MoonIcon as ComponentType<SVGProps<SVGSVGElement>>;

type AppearanceSwitchProps = {
  appearance: AppearanceType;
  setAppearance: (t: AppearanceType) => void;
};

function AppearanceSwitch({
  appearance,
  setAppearance,
}: AppearanceSwitchProps) {
  return (
    <HStack gap={2} vAlign="center" padding={4}>
      <Switch
        label="Dark mode"
        isLabelHidden
        value={appearance === "dark"}
        onChange={(checked) => setAppearance(checked ? "dark" : "light")}
      />
      <Icon icon={MoonSvgIcon} color="secondary" />
    </HStack>
  );
}

export default AppearanceSwitch;
