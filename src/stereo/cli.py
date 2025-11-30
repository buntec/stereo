import argparse
import os
import subprocess

from stereo._version import __version__


def main():
    parser = argparse.ArgumentParser(description="The stereo command-line interface")

    parser.add_argument(
        "--home",
        metavar="PATH",
        help="path to where app files like `stereo.db` file will be stored.",
    )

    parser.add_argument(
        "--port",
        "-p",
        type=int,
        default=8005,
        help="the port on which the server will listen",
    )

    parser.add_argument(
        "--verbose",
        "-v",
        action="count",
        default=0,
        help="increase verbosity (can be used multiple times)",
    )

    parser.add_argument("--version", action="store_true")

    parser.add_argument("--dev", action="store_true", help="run server in dev mode")

    args = parser.parse_args()

    if args.version:
        print(__version__)
        return 0

    env = os.environ

    env.update(
        {
            "STEREO_VERBOSITY": str(args.verbose),
            "STEREO_VERSION": __version__,
            "STEREO_DEV": str(args.dev),
        }
    )

    if args.home:
        env.update({"STEREO_HOME": str(args.home)})

    cmd = ["uvicorn"]

    cmd.extend(["--host", "0.0.0.0", "--port", str(args.port), "--log-level", "info"])

    if args.dev:
        cmd.append("--reload")

    cmd.append("stereo.server:app")

    subprocess.run(cmd, env=env)
