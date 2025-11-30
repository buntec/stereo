import logging.config
from pathlib import Path


def setup_logger(verbosity: int, app_dir: Path):
    level = logging.CRITICAL
    if verbosity < 1:
        level = logging.WARNING
    elif verbosity < 2:
        level = logging.INFO
    else:
        level = logging.DEBUG

    config = {
        "version": 1,
        "disable_existing_loggers": False,
        "formatters": {
            "standard": {
                "format": "%(asctime)s | %(levelname)s | %(name)s | %(message)s"
            },
            "rich": {"datefmt": "[%X]", "format": "%(message)s"},
        },
        "handlers": {
            "console": {
                "class": "rich.logging.RichHandler",
                "formatter": "rich",
                "markup": True,
                "show_path": False,
            },
            "file": {
                "class": "logging.handlers.RotatingFileHandler",
                "formatter": "standard",
                "filename": app_dir / "stereo.log",
                "maxBytes": 10485760,  # 10MB
                "backupCount": 3,
                "encoding": "utf8",
            },
        },
        "root": {
            "handlers": ["console", "file"],
            "level": level,
        },
    }

    logging.config.dictConfig(config)
