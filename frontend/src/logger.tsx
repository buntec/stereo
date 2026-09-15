const APP_PREFIX = "[Stereo]";
type LogArgs = unknown[];

const logger = {
  log: (...args: LogArgs) => console.log(APP_PREFIX, ...args),
  error: (...args: LogArgs) => console.error(APP_PREFIX, ...args),
  warn: (...args: LogArgs) => console.warn(APP_PREFIX, ...args),
  info: (...args: LogArgs) => console.info(APP_PREFIX, ...args),
  debug: (...args: LogArgs) => console.debug(APP_PREFIX, ...args),
};

export default logger;
