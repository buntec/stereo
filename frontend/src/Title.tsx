import { Box, Text } from "@radix-ui/themes";

function Title({
  title,
  artists,
  trackName,
  label,
  releaseDate,
}: {
  title: string;
  artists?: string[];
  trackName?: string;
  label?: string;
  releaseDate?: string;
}) {
  const infoParts = [
    artists && artists.length > 0 ? artists.join(", ") : undefined,
    trackName,
    label,
    releaseDate,
  ].filter(Boolean);

  return (
    <Box className="title">
      <Text size="5" weight="light">
        {title}
      </Text>
      {infoParts.length > 0 && (
        <Text size="2" color="gray" as="div">
          {infoParts.join(" - ")}
        </Text>
      )}
    </Box>
  );
}

export default Title;
