import { Box, Text } from "@radix-ui/themes";

function Title({ title }: { title: string }) {
  return (
    <Box className="title">
      <Text size="5" weight="light">
        {title}
      </Text>
    </Box>
  );
}

export default Title;
