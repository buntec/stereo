import { Box, Text } from "@radix-ui/themes";

function Title({ title }: { title: string }) {
  return (
    <Box className="title" m="4">
      <Text size="5" weight="light">
        {title}
      </Text>
    </Box>
  );
}

export default Title;
