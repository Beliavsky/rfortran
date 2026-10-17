"""Integration check for the actual sandbox image; used before publishing it."""
import argparse
import json
import subprocess
from server import container_command
import uuid


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("image")
    args = parser.parse_args()
    cases = [
        ({"files":[{"name":"main.f90","content":"print *, 42\nend\n"}]}, "42"),
        ({"files":[{"name":"main.f90","content":"integer :: n\nread *, n\nprint *, n*n\nend\n"}],"stdin":"7\n"}, "49"),
        ({"files":[{"name":"values.f90","content":"module values\ninteger, parameter :: n=17\nend module values\n"},
                   {"name":"main.f90","content":"use values\nprint *, n\nend\n"}],"optimization":"2"}, "17"),
    ]
    for request, expected in cases:
        name = "rfort-smoke-" + uuid.uuid4().hex
        try:
            job = subprocess.run(container_command(args.image, name), input=json.dumps(request), text=True, capture_output=True, timeout=40)
            result = json.loads(job.stdout)
            assert result.get("build", {}).get("exit_code") == 0, (result, job.stderr)
            assert result.get("run", {}).get("stdout", "").strip() == expected, result
            print(f"Sandbox smoke passed: {expected}")
        finally:
            subprocess.run(["docker", "rm", "-f", name], capture_output=True, timeout=10)

    name = "rfort-smoke-" + uuid.uuid4().hex
    try:
        request = {"files": [{"name": "main.f90", "content": "program forever\ndo\nend do\nend program forever\n"}]}
        job = subprocess.run(container_command(args.image, name), input=json.dumps(request), text=True, capture_output=True, timeout=40)
        result = json.loads(job.stdout)
        assert result.get("build", {}).get("exit_code") == 0, result
        assert result.get("run", {}).get("timed_out"), result
        print("Sandbox smoke passed: infinite loop terminated")
    finally:
        subprocess.run(["docker", "rm", "-f", name], capture_output=True, timeout=10)


if __name__ == "__main__":
    main()
