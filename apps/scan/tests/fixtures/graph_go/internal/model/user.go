package model

const MaxNameLength = 64

type User struct {
	Name string
}

func (u User) Display() string {
	return u.Name
}

type Storer interface {
	Load() User
}

type ID = string
