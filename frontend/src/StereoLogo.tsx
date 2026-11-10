type StereoLogoProps = {
  size?: number;
};

// Stereo's mark: a horn-loaded PA cabinet (horn panel over a bass horn mouth).
// Same geometry as frontend/public/favicon.svg, without the tile.
function StereoLogo({ size = 24 }: StereoLogoProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 64 64"
      role="img"
      aria-label="Stereo"
    >
      <rect x="11" y="3" width="42" height="58" rx="2.5" fill="#2f2b57" />
      <path d="M19.5 6H44.5L50 11.5V36H14V11.5Z" fill="#d3d5da" />
      <path d="M22 7.5H42L34.5 9.6H29.5Z" fill="#6c717e" />
      <path d="M22 14.6H42L34.5 12.6H29.5Z" fill="#eef0f2" />
      <path
        d="M22 7.5L29.5 9.6V12.6L22 14.6ZM42 7.5L34.5 9.6V12.6L42 14.6Z"
        fill="#a7abb4"
      />
      <rect x="29.5" y="9.6" width="5" height="3" fill="#110f21" />
      <path d="M16 16.5H48L40 20.5H24Z" fill="#6c717e" />
      <path d="M16 34.5H48L40 30.5H24Z" fill="#eef0f2" />
      <path
        d="M16 16.5L24 20.5V30.5L16 34.5ZM48 16.5L40 20.5V30.5L48 34.5Z"
        fill="#a7abb4"
      />
      <rect x="24" y="20.5" width="16" height="10" fill="#110f21" />
      <path d="M20.5 25.5L26 23.3H38L43.5 25.5Z" fill="#eef0f2" />
      <path d="M20.5 25.5L26 27.7H38L43.5 25.5Z" fill="#6c717e" />
      <rect x="14" y="39" width="36" height="19" fill="#110f21" />
      <path
        d="M14 39H50L46 43H18ZM14 39L18 43V58H14ZM50 39L46 43V58H50Z"
        fill="#23203f"
      />
      <path
        d="M31 43H33V58H31ZM25 47H39V48.6H25ZM18 58V54.5L21.5 58ZM46 58V54.5L42.5 58Z"
        fill="#2f2b57"
      />
    </svg>
  );
}

export default StereoLogo;
