import "./App.css";
import { MoonIcon } from "@radix-ui/react-icons";

import { Switch, Box, Flex, Text } from "@radix-ui/themes";
import type { AppearanceType } from "./Types.tsx";

type AppearanceSwitchProps = {
  appearance: AppearanceType;
  setAppearance: (t: AppearanceType) => void;
};

function AppearanceSwitch({
  appearance,
  setAppearance,
}: AppearanceSwitchProps) {
  return (
    <Box m="4">
      <Text as="label" size="2">
        <Flex gap="2" align="center">
          <Switch
            color="gold"
            size="1"
            checked={appearance === "dark"}
            onCheckedChange={(checked) =>
              setAppearance(checked ? "dark" : "light")
            }
          />{" "}
          <MoonIcon />
        </Flex>
      </Text>
    </Box>
  );
}

export default AppearanceSwitch;
