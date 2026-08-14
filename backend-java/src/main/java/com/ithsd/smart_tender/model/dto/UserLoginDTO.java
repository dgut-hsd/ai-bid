package com.ithsd.smart_tender.model.dto;

import com.fasterxml.jackson.annotation.JsonAlias;
import lombok.Data;
import java.io.Serializable;

@Data
public class UserLoginDTO implements Serializable {
    @JsonAlias("username")
    private String phone;
    private String password;

    /** Contract name alias; the legacy service still queries the phone field. */
    public void setUsername(String username) {
        this.phone = username;
    }

    public String getUsername() {
        return phone;
    }
}
