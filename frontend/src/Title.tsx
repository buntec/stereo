import { Box, Text } from "@radix-ui/themes";

function Title({ title, artists }: { title: string; artists?: string[] }) {
  return (
    <Box className="title">
      <Text size="5" weight="light">
        {title}
      </Text>
      {artists && artists.length > 0 && (
        <Text size="2" color="gray" as="div">
          {artists.join(", ")}
        </Text>
      )}
    </Box>
  );
}

export default Title;
