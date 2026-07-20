package report

import "os/exec"

type Report struct {
	Name      string
	Threshold int
}

func (r *Report) Classify(score int, flags []string) string {
	var grade string
	switch {
	case score > 90:
		grade = "A"
	case score > 80:
		grade = "B"
	case score > r.Threshold:
		grade = "C"
	default:
		grade = "F"
	}
	for _, f := range flags {
		if (f == "urgent" || f == "review") && grade != "F" {
			grade = grade + "!"
		}
	}
	return grade
}

func Run(cmd string) ([]byte, error) {
	return exec.Command("sh", "-c", cmd).Output()
}
