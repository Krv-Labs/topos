"""A branchy classifier plus a deliberately risky shell call."""

import os


class Report:
    def __init__(self, name: str, threshold: int = 70) -> None:
        self.name = name
        self.threshold = threshold

    def classify(self, score: int, flags: list[str]) -> str:
        if score > 90:
            grade = "A"
        elif score > 80:
            grade = "B"
        elif score > self.threshold:
            grade = "C"
        else:
            grade = "F"
        if flags and ("urgent" in flags or "review" in flags):
            grade += "!"
        for extra in flags:
            if extra.startswith("bonus") and grade != "F":
                grade = "A+"
                break
        return grade


def run(cmd: str) -> int:
    # dangerous call: exercises the SECURE generator
    return os.system(cmd)
